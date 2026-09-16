using System.Windows;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Shapes;
using System.Windows.Threading;
using ClaudeCodeUsage.App.Services;
using ClaudeCodeUsage.Core.Models;
using Color = System.Windows.Media.Color;
using Point = System.Windows.Point;
using Size = System.Windows.Size;

namespace ClaudeCodeUsage.App.UI;

public partial class WidgetWindow : Window
{
    private readonly AppConfig _config;
    private readonly Action _persist;
    private readonly DispatcherTimer _tick;
    private UsageSnapshot _last;
    private bool _dragging;

    /// <summary>How long without new Claude activity before the widget shows "Stale".</summary>
    private static readonly TimeSpan StaleAfter = TimeSpan.FromMinutes(10);
    /// <summary>Ring radii — must match the Ellipse diameters in the XAML (92 and 68).</summary>
    private const double OuterRadius = 46;
    private const double InnerRadius = 34;
    private const double SnapMargin = 12;
    private const double SnapThreshold = 80;

    public WidgetWindow(AppConfig config, Action persist)
    {
        InitializeComponent();
        _config = config;
        _persist = persist;
        _last = UsageSnapshot.Empty(UsageState.NoData, config.TokenLimit, DateTime.UtcNow);

        Loaded += OnLoaded;

        // Keep relative times ("resets in", "updated Xs ago") live between snapshots.
        _tick = new DispatcherTimer { Interval = TimeSpan.FromSeconds(1) };
        _tick.Tick += (_, _) => Render();
        _tick.Start();
    }

    private void OnLoaded(object sender, RoutedEventArgs e) => RestorePosition();

    public void Update(UsageSnapshot snapshot)
    {
        _last = snapshot;
        Render();
    }

    private void Render()
    {
        var now = DateTime.UtcNow;
        var s = _last;
        bool live = s.State is UsageState.Ok or UsageState.NoActiveSession;

        var sessionColor = live ? UsageVisuals.WpfColorFor(s.Session) : UsageVisuals.Grey;
        var weeklyColor = UsageVisuals.WpfColorFor(s.Weekly);

        // Centre: the session number, because that is the one that bites first.
        PercentText.Text = live ? UsageVisuals.FormatPercent(s.Session) : "—";
        StatusText.Text = ResolveCaption(s, now);

        SetArc(SessionArc, OuterRadius, live ? s.Session?.Utilization ?? 0 : 0, sessionColor);
        SetArc(WeeklyArc, InnerRadius, s.Weekly?.Utilization ?? 0, weeklyColor);

        CompactText.Text = BuildCompactLine(s, now);
        Card.ToolTip = BuildTooltip(s, now);
    }

    /// <summary>
    /// The one visible line under the ring: the weekly figure the centre cannot show,
    /// and how long the session window has left. Either half is dropped when unknown
    /// rather than rendered as a dash.
    /// </summary>
    private static string BuildCompactLine(UsageSnapshot s, DateTime now)
    {
        var parts = new List<string>(2);

        if (s.Weekly is not null)
            parts.Add($"wk {UsageVisuals.FormatPercent(s.Weekly)}");

        if (s.Session?.ResetUtc is not null)
            parts.Add(UsageVisuals.FormatDuration(s.TimeUntilReset(now)) + " left");

        return string.Join("  ·  ", parts);
    }

    /// <summary>
    /// Everything the minimal face leaves out. Hovering is the way back to the detail,
    /// so this must name the data source — an estimate should never be mistaken for the
    /// real figure just because the card is quiet.
    /// </summary>
    private static string BuildTooltip(UsageSnapshot s, DateTime now)
    {
        var lines = new List<string>
        {
            $"Session  {UsageVisuals.FormatPercent(s.Session)}" +
            (s.Session?.ResetUtc is null
                ? ""
                : $"   resets {UsageVisuals.FormatReset(s.Session.ResetUtc, now)}"),
        };

        if (s.Weekly is not null)
            lines.Add($"Weekly   {UsageVisuals.FormatPercent(s.Weekly)}" +
                      (s.Weekly.ResetUtc is null
                          ? ""
                          : $"   resets {UsageVisuals.FormatReset(s.Weekly.ResetUtc, now)}"));

        lines.Add("");
        lines.Add(s.Source == UsageSource.Official
            ? "Official figures from Anthropic"
            : $"Local estimate — {UsageVisuals.FormatTokens(s.TokensUsed)} of {UsageVisuals.FormatTokens(s.TokenLimit)} tokens");

        if (s.LastActivityUtc is not null)
            lines.Add($"Last activity {UsageVisuals.FormatRelative(s.LastActivityUtc, now)}");

        return string.Join(Environment.NewLine, lines);
    }

    /// <summary>The caption under the percent. Blank in the healthy official case — the
    /// minimal face should say nothing when there is nothing to flag.</summary>
    private static string ResolveCaption(UsageSnapshot s, DateTime now)
    {
        if (s.State == UsageState.Ok &&
            s.LastActivityUtc is { } last && (now - last) > StaleAfter)
            return "stale";
        if (s.State == UsageState.Ok)
            return s.Source == UsageSource.Official ? "" : "est.";
        return UsageVisuals.StateCaption(s);
    }

    /// <summary>
    /// Draws a clockwise arc from 12 o'clock on the shared 102x102 ring grid. The two
    /// rings differ only by radius, so one routine serves both.
    /// </summary>
    private static void SetArc(Path target, double radius, double fraction, Color color)
    {
        var pct = Math.Clamp(fraction, 0, 1);
        if (pct <= 0)
        {
            target.Data = null;
            return;
        }

        const double cx = 51, cy = 51; // centre of the 102x102 ring grid
        double angle = Math.Min(pct * 360, 359.999);
        double theta = angle * Math.PI / 180.0;

        var start = new Point(cx, cy - radius);
        var end = new Point(cx + radius * Math.Sin(theta), cy - radius * Math.Cos(theta));

        var figure = new PathFigure { StartPoint = start, IsClosed = false };
        figure.Segments.Add(new ArcSegment
        {
            Point = end,
            Size = new Size(radius, radius),
            SweepDirection = SweepDirection.Clockwise,
            IsLargeArc = angle > 180,
        });
        target.Data = new PathGeometry(new[] { figure });
        target.Stroke = new SolidColorBrush(color);
    }

    // ---- Dragging / position ----

    private void OnDragStart(object sender, MouseButtonEventArgs e)
    {
        if (e.ButtonState != MouseButtonState.Pressed) return;
        _dragging = true;
        DragMove();
    }

    private void OnDragEnd(object sender, MouseButtonEventArgs e)
    {
        if (!_dragging) return;
        _dragging = false;
        SnapToNearestCorner();
        _config.WidgetLeft = Left;
        _config.WidgetTop = Top;
        _persist();
    }

    private void OnCloseClick(object sender, MouseButtonEventArgs e)
    {
        e.Handled = true;
        Hide();
        _config.WidgetVisible = false;
        _persist();
    }

    private void RestorePosition()
    {
        var wa = SystemParameters.WorkArea;
        if (double.IsNaN(_config.WidgetLeft) || double.IsNaN(_config.WidgetTop))
        {
            // Default: bottom-right corner.
            Left = wa.Right - Width - SnapMargin;
            Top = wa.Bottom - Height - SnapMargin;
            return;
        }

        Left = _config.WidgetLeft;
        Top = _config.WidgetTop;
        EnsureOnScreen();
    }

    /// <summary>Snap to a screen corner when released near one; otherwise leave in place.</summary>
    private void SnapToNearestCorner()
    {
        var wa = SystemParameters.WorkArea;
        bool nearLeft = Left - wa.Left < SnapThreshold;
        bool nearRight = wa.Right - (Left + Width) < SnapThreshold;
        bool nearTop = Top - wa.Top < SnapThreshold;
        bool nearBottom = wa.Bottom - (Top + Height) < SnapThreshold;

        if (nearLeft) Left = wa.Left + SnapMargin;
        else if (nearRight) Left = wa.Right - Width - SnapMargin;
        if (nearTop) Top = wa.Top + SnapMargin;
        else if (nearBottom) Top = wa.Bottom - Height - SnapMargin;

        EnsureOnScreen();
    }

    private void EnsureOnScreen()
    {
        var wa = SystemParameters.WorkArea;
        Left = Math.Max(wa.Left, Math.Min(Left, wa.Right - Width));
        Top = Math.Max(wa.Top, Math.Min(Top, wa.Bottom - Height));
    }
}
