using System.IO;
using System.Windows.Threading;
using ClaudeCodeUsage.Core;
using ClaudeCodeUsage.Core.Models;

namespace ClaudeCodeUsage.App.Services;

/// <summary>
/// Produces <see cref="UsageSnapshot"/>s from Anthropic's official utilization endpoint,
/// falling back to the local-log estimate whenever that endpoint is unavailable.
/// Watches the Claude logs directory to know when to refresh (and for the "updated Xs
/// ago" freshness line, which the endpoint does not provide).
/// Event-driven (a <see cref="FileSystemWatcher"/> on <c>projects\</c>) with a 60s
/// timer as a safety net, so idle CPU stays near zero. File-change bursts are
/// debounced; a read never runs on the UI thread. <see cref="SnapshotUpdated"/> is
/// always raised on the supplied dispatcher (i.e. the UI thread).
/// </summary>
public sealed class UsageMonitor : IDisposable
{
    private readonly Func<AppConfig> _configProvider;
    private readonly Dispatcher _dispatcher;
    private readonly Action<string>? _log;

    private readonly OAuthUsageClient _official = new();

    /// <summary>
    /// The usage endpoint rate-limits hard, and log writes can fire a refresh every couple
    /// of seconds, so the official figures are cached: fetched at most once per
    /// <see cref="MinFetchInterval"/>, and kept serving for <see cref="CacheTtl"/> after a
    /// failure so one blip does not flip the widget over to the local estimate. Five
    /// minutes is ample resolution for a 5-hour and a 7-day window.
    /// </summary>
    private static readonly TimeSpan MinFetchInterval = TimeSpan.FromMinutes(5);
    private static readonly TimeSpan CacheTtl = TimeSpan.FromMinutes(15);
    private OfficialUsage? _officialCache;
    private DateTime _officialFetchedUtc = DateTime.MinValue;

    private FileSystemWatcher? _watcher;
    private readonly DispatcherTimer _pollTimer;
    private readonly System.Threading.Timer _debounce;
    private readonly object _gate = new();
    private bool _busy;
    private string? _watchedDir;

    public event Action<UsageSnapshot>? SnapshotUpdated;

    public UsageMonitor(Func<AppConfig> configProvider, Dispatcher dispatcher, Action<string>? log = null)
    {
        _configProvider = configProvider;
        _dispatcher = dispatcher;
        _log = log;

        _pollTimer = new DispatcherTimer { Interval = TimeSpan.FromSeconds(60) };
        _pollTimer.Tick += (_, _) => TriggerRefresh();
        _debounce = new System.Threading.Timer(_ => TriggerRefresh(), null,
            Timeout.Infinite, Timeout.Infinite);
    }

    public void Start()
    {
        Reconfigure();
        _pollTimer.Start();
        TriggerRefresh();
    }

    /// <summary>Re-point the file watcher after settings change the Claude directory.</summary>
    public void Reconfigure()
    {
        var claudeDir = UsageReader.ResolveClaudeDir(_configProvider().ClaudeDir);
        var projects = UsageReader.ProjectsDir(claudeDir);

        if (string.Equals(projects, _watchedDir, StringComparison.OrdinalIgnoreCase)
            && _watcher is not null)
            return;

        _watcher?.Dispose();
        _watcher = null;
        _watchedDir = projects;

        if (!Directory.Exists(projects))
        {
            _log?.Invoke($"Projects dir not found yet: {projects} (timer will keep checking)");
            return;
        }

        try
        {
            _watcher = new FileSystemWatcher(projects, "*.jsonl")
            {
                IncludeSubdirectories = true,
                NotifyFilter = NotifyFilters.LastWrite | NotifyFilters.FileName | NotifyFilters.Size,
            };
            _watcher.Changed += OnFsEvent;
            _watcher.Created += OnFsEvent;
            _watcher.Renamed += OnFsEvent;
            _watcher.EnableRaisingEvents = true;
        }
        catch (Exception ex)
        {
            _log?.Invoke($"Could not watch {projects}: {ex.Message}");
            _watcher = null;
        }
    }

    private void OnFsEvent(object sender, FileSystemEventArgs e) => ScheduleDebounced();

    /// <summary>Coalesce a burst of writes into a single refresh ~1.5s after the last one.</summary>
    private void ScheduleDebounced() => _debounce.Change(1500, Timeout.Infinite);

    /// <summary>Kick off a read on the threadpool (never blocks the UI). Overlapping
    /// requests collapse into one in-flight read.</summary>
    public void TriggerRefresh()
    {
        lock (_gate)
        {
            if (_busy) return;
            _busy = true;
        }

        Task.Run(async () =>
        {
            UsageSnapshot snapshot;
            try
            {
                snapshot = await ComputeAsync().ConfigureAwait(false);
            }
            catch (Exception ex)
            {
                _log?.Invoke($"Refresh failed: {ex.Message}");
                snapshot = UsageSnapshot.Empty(UsageState.NoData, _configProvider().TokenLimit,
                    DateTime.UtcNow);
            }
            finally
            {
                lock (_gate) _busy = false;
            }

            await _dispatcher.BeginInvoke(() => SnapshotUpdated?.Invoke(snapshot));
        });
    }

    /// <summary>
    /// Reads the logs (for freshness and as the fallback), then overlays Anthropic's
    /// official percentages when the endpoint answers. A failed or disabled fetch simply
    /// leaves the local estimate in place.
    /// </summary>
    private async Task<UsageSnapshot> ComputeAsync()
    {
        var cfg = _configProvider();
        var claudeDir = UsageReader.ResolveClaudeDir(cfg.ClaudeDir);
        var result = UsageReader.ReadEvents(claudeDir, m => _log?.Invoke(m));
        var local = UsageCalculator.Compute(result.Events, cfg.TokenLimit, cfg.WindowHours,
            DateTime.UtcNow, result.LogsFound);

        if (!cfg.UseOfficialUsage) return local;

        var official = await GetOfficialAsync(claudeDir).ConfigureAwait(false);
        if (official?.Session is null) return local;

        return new UsageSnapshot
        {
            State = UsageState.Ok,
            Source = UsageSource.Official,
            Session = official.Session,
            Weekly = official.Weekly,
            TokensUsed = local.TokensUsed,
            TokenLimit = cfg.TokenLimit,
            LastActivityUtc = local.LastActivityUtc,
            GeneratedAtUtc = DateTime.UtcNow,
            EventsCounted = local.EventsCounted,
        };
    }

    /// <summary>Returns the official figures, honouring the fetch interval and serving the
    /// last good value while it is still fresh enough.</summary>
    private async Task<OfficialUsage?> GetOfficialAsync(string claudeDir)
    {
        var now = DateTime.UtcNow;
        var age = now - _officialFetchedUtc;

        if (_officialCache is not null && age < MinFetchInterval)
            return _officialCache;

        var fetched = await _official.FetchAsync(claudeDir, m => _log?.Invoke(m))
            .ConfigureAwait(false);

        if (fetched is not null)
        {
            _officialCache = fetched;
            _officialFetchedUtc = now;
            return fetched;
        }

        // Fetch failed. Keep showing the last good figures until they go properly stale.
        if (_officialCache is not null && age < CacheTtl) return _officialCache;

        _officialCache = null;
        return null;
    }

    public void Dispose()
    {
        _pollTimer.Stop();
        _watcher?.Dispose();
        _debounce.Dispose();
        _official.Dispose();
    }
}
