using ClaudeCodeUsage.Core.Models;
using Color = System.Windows.Media.Color;
using Drawing = System.Drawing;

namespace ClaudeCodeUsage.App.Services;

/// <summary>Shared formatting + colour rules used by both the tray icon and the widget,
/// so they never disagree. The ring is a traffic light on how much of the window is gone:
/// green &lt; 70%, orange 70–90%, red &gt; 90%.</summary>
public static class UsageVisuals
{
    // WPF colours (widget).
    /// <summary>Plenty left (under 70% of the window used).</summary>
    public static readonly Color Green = Color.FromRgb(0x33, 0xC0, 0x59);
    /// <summary>Getting close (70–90%).</summary>
    public static readonly Color Orange = Color.FromRgb(0xEE, 0x80, 0x33);
    /// <summary>Nearly exhausted (over 90%).</summary>
    public static readonly Color Red = Color.FromRgb(0xE0, 0x40, 0x40);
    public static readonly Color Grey = Color.FromRgb(0x88, 0x88, 0x88);

    /// <summary>Threshold colour for a single window. Grey when the window is unknown.</summary>
    public static Color WpfColorFor(UsageWindow? window)
    {
        if (window is null) return Grey;
        return window.Utilization switch
        {
            < 0.70 => Green,
            < 0.90 => Orange,
            _ => Red,
        };
    }

    /// <summary>Colour of the snapshot's session window — the app's primary signal.</summary>
    public static Color WpfColorFor(UsageSnapshot s) =>
        s.State == UsageState.Ok ? WpfColorFor(s.Session) : Grey;

    // System.Drawing colours (tray icon) — same rules, mirrored.
    public static Drawing.Color DrawingColorFor(UsageSnapshot s) => ToDrawing(WpfColorFor(s));

    public static Drawing.Color ToDrawing(Color c) => Drawing.Color.FromArgb(c.R, c.G, c.B);

    /// <summary>Compact token count, e.g. 8.3k, 2.1M.</summary>
    public static string FormatTokens(long tokens)
    {
        if (tokens >= 1_000_000)
            return (tokens / 1_000_000.0).ToString("0.##") + "M";
        if (tokens >= 1_000)
            return (tokens / 1_000.0).ToString("0.#") + "k";
        return tokens.ToString("0");
    }

    /// <summary>"42%" for a known window, "—" otherwise.</summary>
    public static string FormatPercent(UsageWindow? window) =>
        window is null ? "—" : $"{Math.Round(window.Utilization * 100)}%";

    public static string FormatPercent(UsageSnapshot s) =>
        s.State == UsageState.Ok ? FormatPercent(s.Session) : "—";

    /// <summary>"1h 47m", "12m", or "0m".</summary>
    public static string FormatDuration(TimeSpan? span)
    {
        if (span is null) return "—";
        var t = span.Value;
        if (t.TotalHours >= 1)
            return $"{(int)t.TotalHours}h {t.Minutes}m";
        return $"{t.Minutes}m";
    }

    /// <summary>
    /// Reset text scaled to the distance: minutes/hours for the session window,
    /// a weekday and time once it is more than a day out (as the weekly window is).
    /// </summary>
    public static string FormatReset(DateTime? resetUtc, DateTime nowUtc)
    {
        if (resetUtc is null) return "—";
        var remaining = resetUtc.Value - nowUtc;
        if (remaining < TimeSpan.Zero) remaining = TimeSpan.Zero;
        if (remaining.TotalHours < 24) return $"in {FormatDuration(remaining)}";
        return resetUtc.Value.ToLocalTime().ToString("ddd h:mmtt").ToLowerInvariant();
    }

    /// <summary>Relative "updated 12s ago" style text.</summary>
    public static string FormatRelative(DateTime? whenUtc, DateTime nowUtc)
    {
        if (whenUtc is null) return "never";
        var d = nowUtc - whenUtc.Value;
        if (d < TimeSpan.Zero) d = TimeSpan.Zero;
        if (d.TotalSeconds < 60) return $"{(int)d.TotalSeconds}s ago";
        if (d.TotalMinutes < 60) return $"{(int)d.TotalMinutes}m ago";
        if (d.TotalHours < 24) return $"{(int)d.TotalHours}h ago";
        return $"{(int)d.TotalDays}d ago";
    }

    /// <summary>Human status line for the given snapshot state.</summary>
    public static string StateCaption(UsageSnapshot s) => s.State switch
    {
        UsageState.Ok => s.Source == UsageSource.Official ? "session" : "session (est.)",
        UsageState.NoActiveSession => "No active session",
        UsageState.NoData => "No data yet",
        UsageState.NoLogsFound => "No .claude logs found",
        _ => "",
    };
}
