namespace ClaudeCodeUsage.Core.Models;

/// <summary>
/// One rate-limit window (the 5-hour session window, or the 7-day weekly window) as a
/// fraction consumed plus when it resets. Both the official endpoint and the local-log
/// fallback produce these, so the UI renders one shape regardless of source.
/// </summary>
public sealed class UsageWindow
{
    /// <summary>Fraction of the window consumed (0..1+, can exceed 1).</summary>
    public double Utilization { get; init; }

    /// <summary>When this window resets (UTC), if known.</summary>
    public DateTime? ResetUtc { get; init; }

    /// <summary>Time remaining until reset. Null when unknown; never negative.</summary>
    public TimeSpan? TimeUntilReset(DateTime nowUtc)
    {
        if (ResetUtc is null) return null;
        var remaining = ResetUtc.Value - nowUtc;
        return remaining < TimeSpan.Zero ? TimeSpan.Zero : remaining;
    }
}

/// <summary>Where a snapshot's numbers came from.</summary>
public enum UsageSource
{
    /// <summary>Anthropic's own utilization endpoint — the same numbers <c>/usage</c> shows.</summary>
    Official,
    /// <summary>Derived locally from Claude Code session logs against a configured token limit.</summary>
    LocalLogs,
}

/// <summary>Session + weekly windows as returned by the official usage endpoint.</summary>
public sealed record OfficialUsage(UsageWindow? Session, UsageWindow? Weekly);
