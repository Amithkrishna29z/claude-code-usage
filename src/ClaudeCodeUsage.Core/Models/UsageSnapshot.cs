namespace ClaudeCodeUsage.Core.Models;

/// <summary>Overall health of the most recent snapshot.</summary>
public enum UsageState
{
    /// <summary>Usable numbers are present for the session window.</summary>
    Ok,
    /// <summary>Logs were found and parsed, but the last window has fully elapsed
    /// with no recent activity — usage has effectively reset to zero.</summary>
    NoActiveSession,
    /// <summary>Logs were found but contained no parseable usage events.</summary>
    NoData,
    /// <summary>The Claude logs directory does not exist / has no *.jsonl files.</summary>
    NoLogsFound,
}

/// <summary>
/// Immutable, UI-ready summary of Claude Code usage. <see cref="Session"/> and
/// <see cref="Weekly"/> are the display surface; the token fields below them are only
/// meaningful when <see cref="Source"/> is <see cref="UsageSource.LocalLogs"/>.
/// </summary>
public sealed class UsageSnapshot
{
    public UsageState State { get; init; } = UsageState.NoData;

    /// <summary>Whether these numbers are Anthropic's own or locally derived.</summary>
    public UsageSource Source { get; init; } = UsageSource.LocalLogs;

    /// <summary>The 5-hour session window. Null when no usable data.</summary>
    public UsageWindow? Session { get; init; }

    /// <summary>The 7-day weekly window. Only the official source supplies this.</summary>
    public UsageWindow? Weekly { get; init; }

    /// <summary>Tokens counted in the active block (local-log source only).</summary>
    public long TokensUsed { get; init; }

    /// <summary>Configured per-window token budget (local-log source only).</summary>
    public long TokenLimit { get; init; }

    /// <summary>Fraction of the session window consumed (0..1+). 0 when unknown.</summary>
    public double PercentUsed => Session?.Utilization ?? 0;

    /// <summary>Start of the active 5-hour block (local-log source only).</summary>
    public DateTime? WindowStartUtc { get; init; }

    /// <summary>When the session window resets, if known.</summary>
    public DateTime? ResetUtc => Session?.ResetUtc;

    /// <summary>Timestamp of the most recent local usage event (UTC), if any. Drives
    /// the "updated Xs ago" line and stale detection, and is read from the logs even
    /// when the official source supplies the percentages.</summary>
    public DateTime? LastActivityUtc { get; init; }

    /// <summary>When this snapshot was computed (UTC).</summary>
    public DateTime GeneratedAtUtc { get; init; }

    /// <summary>Number of usage events counted toward <see cref="TokensUsed"/>.</summary>
    public int EventsCounted { get; init; }

    /// <summary>Time remaining until the session window resets, relative to
    /// <paramref name="nowUtc"/>. Null when unknown. Never negative.</summary>
    public TimeSpan? TimeUntilReset(DateTime nowUtc) => Session?.TimeUntilReset(nowUtc);

    public static UsageSnapshot Empty(UsageState state, long tokenLimit, DateTime nowUtc) => new()
    {
        State = state,
        TokenLimit = tokenLimit,
        GeneratedAtUtc = nowUtc,
    };
}
