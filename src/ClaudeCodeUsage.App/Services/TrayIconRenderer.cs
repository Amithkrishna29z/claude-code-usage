using System.Drawing;
using System.Drawing.Drawing2D;
using System.Runtime.InteropServices;
using ClaudeCodeUsage.Core.Models;

namespace ClaudeCodeUsage.App.Services;

/// <summary>Draws the tray icon dynamically: two concentric colour-coded rings, outer
/// for the 5-hour session window and inner for the 7-day weekly window, mirroring the
/// widget. Colours follow the same thresholds (green/amber/red), grey when unknown.
/// The inner ring is simply absent when there is no weekly figure.</summary>
public static class TrayIconRenderer
{
    [DllImport("user32.dll", SetLastError = true)]
    private static extern bool DestroyIcon(IntPtr handle);

    /// <summary>Frees an HICON previously returned via <paramref name="hIcon"/> from
    /// <see cref="Render"/>. Call on the icon you are replacing to avoid handle leaks.</summary>
    public static void ReleaseIcon(IntPtr hIcon)
    {
        if (hIcon != IntPtr.Zero) DestroyIcon(hIcon);
    }

    public static Icon Render(UsageSnapshot s, out IntPtr hIcon)
    {
        const int size = 32;
        using var bmp = new Bitmap(size, size);
        using (var g = Graphics.FromImage(bmp))
        {
            g.SmoothingMode = SmoothingMode.AntiAlias;
            g.Clear(Color.Transparent);

            bool live = s.State == UsageState.Ok;
            var sessionColor = UsageVisuals.DrawingColorFor(s);

            // Outer ring: session.
            var outer = new RectangleF(3, 3, size - 6, size - 6);
            DrawRing(g, outer, 4, live ? s.Session?.Utilization ?? 0 : 0, sessionColor);

            // Inner ring: weekly (drawn only when the official source supplied it).
            if (s.Weekly is { } weekly)
            {
                var inner = new RectangleF(9.5f, 9.5f, size - 19, size - 19);
                var weeklyColor = UsageVisuals.ToDrawing(UsageVisuals.WpfColorFor(weekly));
                DrawRing(g, inner, 3, weekly.Utilization, weeklyColor);
            }
            else
            {
                // Centre dot conveys state at tiny sizes even when the arc is short.
                using var dot = new SolidBrush(sessionColor);
                g.FillEllipse(dot, size / 2f - 4, size / 2f - 4, 8, 8);
            }
        }

        hIcon = bmp.GetHicon();
        // Clone into a managed Icon so the NotifyIcon owns a copy independent of the HICON.
        using var fromHandle = Icon.FromHandle(hIcon);
        return (Icon)fromHandle.Clone();
    }

    /// <summary>Track plus a clockwise progress arc starting at 12 o'clock.</summary>
    private static void DrawRing(Graphics g, RectangleF rect, float width, double fraction, Color color)
    {
        using (var track = new Pen(Color.FromArgb(70, 90, 90, 90), width))
            g.DrawEllipse(track, rect);

        var pct = Math.Clamp(fraction, 0, 1);
        if (pct <= 0) return;

        using var arc = new Pen(color, width) { StartCap = LineCap.Round, EndCap = LineCap.Round };
        g.DrawArc(arc, rect, -90f, (float)(360 * pct));
    }
}
