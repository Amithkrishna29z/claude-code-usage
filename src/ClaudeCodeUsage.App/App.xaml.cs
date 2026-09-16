using System.Windows;
using ClaudeCodeUsage.App.Services;
using ClaudeCodeUsage.App.UI;
using ClaudeCodeUsage.Core;
using ClaudeCodeUsage.Core.Models;
using WinForms = System.Windows.Forms;
using Application = System.Windows.Application;
using MessageBox = System.Windows.MessageBox;
using MessageBoxButton = System.Windows.MessageBoxButton;
using MessageBoxImage = System.Windows.MessageBoxImage;

namespace ClaudeCodeUsage.App;

/// <summary>
/// Application entry point and orchestrator. Owns the tray icon, the mini widget,
/// and the <see cref="UsageMonitor"/>, and keeps them in sync as snapshots arrive.
/// There is no main window — the app lives in the tray.
/// </summary>
public partial class App : Application
{
    private Mutex? _singleInstance;
    private ConfigService _configService = null!;
    private AppConfig _config = null!;
    private UsageMonitor _monitor = null!;
    private WidgetWindow? _widget;

    private WinForms.NotifyIcon _tray = null!;
    private WinForms.ToolStripMenuItem _widgetItem = null!;
    private WinForms.ToolStripMenuItem _startupItem = null!;
    private IntPtr _lastHIcon = IntPtr.Zero;
    private UsageSnapshot _lastSnapshot = UsageSnapshot.Empty(UsageState.NoData, 0, DateTime.UtcNow);

    protected override void OnStartup(StartupEventArgs e)
    {
        base.OnStartup(e);

        _singleInstance = new Mutex(initiallyOwned: true, "ClaudeCodeUsage.SingleInstance", out var isNew);
        if (!isNew)
        {
            Shutdown();
            return;
        }

        _configService = new ConfigService();
        _config = _configService.Load();
        // Reflect the real registry state so the menu check is honest.
        _config.StartWithWindows = StartupService.IsEnabled();

        BuildTray();

        _monitor = new UsageMonitor(() => _config, Dispatcher);
        _monitor.SnapshotUpdated += OnSnapshot;

        _widget = new WidgetWindow(_config, PersistConfig);
        if (_config.WidgetVisible) _widget.Show();

        _monitor.Start();
    }

    private void BuildTray()
    {
        var menu = new WinForms.ContextMenuStrip();

        _widgetItem = new WinForms.ToolStripMenuItem("Show widget", null, (_, _) => ToggleWidget());
        var settingsItem = new WinForms.ToolStripMenuItem("Settings…", null, (_, _) => OpenSettings());
        var refreshItem = new WinForms.ToolStripMenuItem("Refresh now", null, (_, _) => _monitor.TriggerRefresh());
        _startupItem = new WinForms.ToolStripMenuItem("Start with Windows", null, (_, _) => ToggleStartup())
        {
            Checked = _config.StartWithWindows,
        };
        var quitItem = new WinForms.ToolStripMenuItem("Quit", null, (_, _) => ExitApp());

        menu.Items.Add(_widgetItem);
        menu.Items.Add(settingsItem);
        menu.Items.Add(refreshItem);
        menu.Items.Add(new WinForms.ToolStripSeparator());
        menu.Items.Add(_startupItem);
        menu.Items.Add(new WinForms.ToolStripSeparator());
        menu.Items.Add(quitItem);

        _tray = new WinForms.NotifyIcon
        {
            Text = "Claude Code Usage",
            Visible = true,
            ContextMenuStrip = menu,
            Icon = TrayIconRenderer.Render(_lastSnapshot, out _lastHIcon),
        };
        _tray.MouseClick += (_, args) =>
        {
            if (args.Button == WinForms.MouseButtons.Left) ToggleWidget();
        };
        _tray.DoubleClick += (_, _) => ToggleWidget();
    }

    private void OnSnapshot(UsageSnapshot snapshot)
    {
        _lastSnapshot = snapshot;
        _widget?.Update(snapshot);
        UpdateTray(snapshot);
    }

    private void UpdateTray(UsageSnapshot s)
    {
        // Swap the icon, then free the previous native handle.
        var oldIcon = _tray.Icon;
        var oldHIcon = _lastHIcon;
        _tray.Icon = TrayIconRenderer.Render(s, out _lastHIcon);
        oldIcon?.Dispose();
        TrayIconRenderer.ReleaseIcon(oldHIcon);

        _tray.Text = BuildTooltip(s);
        _widgetItem.Text = (_widget?.IsVisible ?? false) ? "Hide widget" : "Show widget";
    }

    /// <summary>NotifyIcon.Text is capped at 63 chars — keep it compact.</summary>
    private static string BuildTooltip(UsageSnapshot s)
    {
        var now = DateTime.UtcNow;
        if (s.State != UsageState.Ok)
            return $"Claude Code Usage — {UsageVisuals.StateCaption(s)}";

        var line = $"Session {UsageVisuals.FormatPercent(s.Session)}" +
                   $" • resets in {UsageVisuals.FormatDuration(s.TimeUntilReset(now))}";

        if (s.Weekly is not null)
            line += Environment.NewLine +
                    $"Weekly {UsageVisuals.FormatPercent(s.Weekly)}" +
                    $" • {UsageVisuals.FormatReset(s.Weekly.ResetUtc, now)}";

        return line;
    }

    private void ToggleWidget()
    {
        if (_widget is null) return;
        if (_widget.IsVisible)
        {
            _widget.Hide();
            _config.WidgetVisible = false;
        }
        else
        {
            _widget.Update(_lastSnapshot);
            _widget.Show();
            _widget.Activate();
            _config.WidgetVisible = true;
        }
        _widgetItem.Text = _widget.IsVisible ? "Hide widget" : "Show widget";
        PersistConfig();
    }

    private void OpenSettings()
    {
        var dialog = new SettingsWindow(_config, _configService.ConfigPath);
        if (dialog.ShowDialog() == true)
        {
            StartupService.Set(_config.StartWithWindows);
            _startupItem.Checked = _config.StartWithWindows;
            PersistConfig();
            _monitor.Reconfigure();
            _monitor.TriggerRefresh();
        }
    }

    private void ToggleStartup()
    {
        var target = !_config.StartWithWindows;
        if (StartupService.Set(target))
        {
            _config.StartWithWindows = target;
            _startupItem.Checked = target;
            PersistConfig();
        }
        else
        {
            MessageBox.Show("Could not update the 'Start with Windows' setting.",
                "Claude Code Usage", MessageBoxButton.OK, MessageBoxImage.Warning);
        }
    }

    private void PersistConfig()
    {
        try { _configService.Save(_config); }
        catch { /* non-fatal: settings just won't persist this time */ }
    }

    private void ExitApp()
    {
        PersistConfig();
        Shutdown();
    }

    protected override void OnExit(ExitEventArgs e)
    {
        _monitor?.Dispose();
        if (_tray is not null)
        {
            _tray.Visible = false;
            _tray.Icon?.Dispose();
            _tray.Dispose();
        }
        TrayIconRenderer.ReleaseIcon(_lastHIcon);
        _singleInstance?.Dispose();
        base.OnExit(e);
    }
}
