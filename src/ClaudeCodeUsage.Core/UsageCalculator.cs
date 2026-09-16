using ClaudeCodeUsage.Core.Models;

namespace ClaudeCodeUsage.Core;

/// <summary>
/// Turns a set of <see cref="UsageEvent"/>s into a <see cref="UsageSnapshot"/> using
/// Anthropic's 5-hour session-block model:
///
///   * Events are grouped into blocks. A block opens at its first event and lasts
///     WINDOW_HOURS. A new block opens when an event is more than WINDOW_HOURS after
///     the current block's start, OR more than WINDOW_HOURS after the previous event
///     (the ">5h gap" rule from the spec).
///   * The active block is the most recent one whose window still covers "now"
///     (start &lt;= now &lt; start + WINDOW_HOURS). Its events determine tokens used
///     and its <c>start + WINDOW_HOURS</c> is the reset time.
///   * If the most recent block's window has fully elapsed, usage has reset: the
///     snapshot reports <see cref="UsageState.NoActiveSession"/> with zero tokens.
/// </summary>
public static class UsageCalculator
{
    public static UsageSnapshot Compute(
        IReadOnlyList<UsageEvent> events,
        long tokenLimit,
        double windowHours,
        DateTime nowUtc,
        bool logsFound = true)
    {
        if (!logsFound)
            return UsageSnapshot.Empty(UsageState.NoLogsFound, tokenLimit, nowUtc);

        if (events.Count == 0)
            return UsageSnapshot.Empty(UsageState.NoData, tokenLimit, nowUtc);

        var window = TimeSpan.FromHours(windowHours <= 0 ? 5.0 : windowHours);
        var ordered = events.OrderBy(e => e.TimestampUtc).ToList();
        var lastActivity = ordered[^1].TimestampUtc;

        // Walk events, accumulating the currently-open block. We only need the block
        // that is active "now", so we track the open block's start and running totals.
        DateTime blockStart = ordered[0].TimestampUtc;
        DateTime prev = blockStart;
        long blockTokens = 0;
        int blockCount = 0;

        foreach (var e in ordered)
        {
            bool newBlock = (e.TimestampUtc - blockStart) > window
                            || (e.TimestampUtc - prev) > window;
            if (newBlock)
            {
                blockStart = e.TimestampUtc;
                blockTokens = 0;
                blockCount = 0;
            }

            blockTokens += e.TotalTokens;
            blockCount++;
            prev = e.TimestampUtc;
        }

        var resetUtc = blockStart + window;

        // Is the final block still active at "now"? If now is past its reset, the
        // window has elapsed with no new activity — usage has reset to zero.
        if (nowUtc >= resetUtc)
        {
            return new UsageSnapshot
            {
                State = UsageState.NoActiveSession,
                Source = UsageSource.LocalLogs,
                Session = new UsageWindow { Utilization = 0 },
                TokensUsed = 0,
                TokenLimit = tokenLimit,
                LastActivityUtc = lastActivity,
                GeneratedAtUtc = nowUtc,
                EventsCounted = 0,
            };
        }

        return new UsageSnapshot
        {
            State = UsageState.Ok,
            Source = UsageSource.LocalLogs,
            Session = new UsageWindow
            {
                Utilization = tokenLimit <= 0 ? 0 : (double)blockTokens / tokenLimit,
                ResetUtc = resetUtc,
            },
            TokensUsed = blockTokens,
            TokenLimit = tokenLimit,
            WindowStartUtc = blockStart,
            LastActivityUtc = lastActivity,
            GeneratedAtUtc = nowUtc,
            EventsCounted = blockCount,
        };
    }
}
