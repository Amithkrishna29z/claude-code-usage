using ClaudeCodeUsage.Core;
using ClaudeCodeUsage.Core.Models;

namespace ClaudeCodeUsage.Tests;

public class UsageCalculatorTests
{
    private static UsageEvent Evt(string utc, long total) => new()
    {
        TimestampUtc = DateTime.Parse(utc).ToUniversalTime(),
        InputTokens = total, // put the whole amount in one component; TotalTokens sums them
    };

    [Fact]
    public void Active_window_starts_after_a_gap_greater_than_five_hours()
    {
        var events = new List<UsageEvent>
        {
            Evt("2026-08-15T00:00:00Z", 1350), // block A — excluded (10h gap follows)
            Evt("2026-08-15T10:00:00Z", 7500), // block B — active
            Evt("2026-08-15T10:30:00Z", 800),  // block B
        };
        var now = DateTime.Parse("2026-08-15T11:00:00Z").ToUniversalTime();

        var snap = UsageCalculator.Compute(events, tokenLimit: 20_000, windowHours: 5, nowUtc: now);

        Assert.Equal(UsageState.Ok, snap.State);
        Assert.Equal(8300, snap.TokensUsed);
        Assert.Equal(2, snap.EventsCounted);
        Assert.Equal(DateTime.Parse("2026-08-15T10:00:00Z").ToUniversalTime(), snap.WindowStartUtc);
        Assert.Equal(DateTime.Parse("2026-08-15T15:00:00Z").ToUniversalTime(), snap.ResetUtc);
        Assert.Equal(TimeSpan.FromHours(4), snap.TimeUntilReset(now));
    }

    [Fact]
    public void Percent_used_is_tokens_over_limit()
    {
        var events = new List<UsageEvent> { Evt("2026-08-15T10:00:00Z", 5000) };
        var now = DateTime.Parse("2026-08-15T11:00:00Z").ToUniversalTime();

        var snap = UsageCalculator.Compute(events, tokenLimit: 20_000, windowHours: 5, nowUtc: now);

        Assert.Equal(0.25, snap.PercentUsed, precision: 6);
    }

    [Fact]
    public void Window_that_has_fully_elapsed_reports_no_active_session()
    {
        var events = new List<UsageEvent> { Evt("2026-08-15T10:00:00Z", 5000) };
        var now = DateTime.Parse("2026-08-15T16:00:00Z").ToUniversalTime(); // past 15:00 reset

        var snap = UsageCalculator.Compute(events, tokenLimit: 20_000, windowHours: 5, nowUtc: now);

        Assert.Equal(UsageState.NoActiveSession, snap.State);
        Assert.Equal(0, snap.TokensUsed);
        Assert.Null(snap.TimeUntilReset(now));
        Assert.Equal(events[0].TimestampUtc, snap.LastActivityUtc);
    }

    [Fact]
    public void No_events_reports_no_data()
    {
        var snap = UsageCalculator.Compute(new List<UsageEvent>(), 20_000, 5,
            DateTime.UtcNow);
        Assert.Equal(UsageState.NoData, snap.State);
    }

    [Fact]
    public void Logs_not_found_is_surfaced()
    {
        var snap = UsageCalculator.Compute(new List<UsageEvent>(), 20_000, 5,
            DateTime.UtcNow, logsFound: false);
        Assert.Equal(UsageState.NoLogsFound, snap.State);
    }

    [Fact]
    public void Time_until_reset_never_negative()
    {
        var events = new List<UsageEvent> { Evt("2026-08-15T10:00:00Z", 5000) };
        var now = DateTime.Parse("2026-08-15T14:59:00Z").ToUniversalTime();

        var snap = UsageCalculator.Compute(events, 20_000, 5, now);
        var remaining = snap.TimeUntilReset(now);

        Assert.NotNull(remaining);
        Assert.True(remaining!.Value >= TimeSpan.Zero);
    }
}
