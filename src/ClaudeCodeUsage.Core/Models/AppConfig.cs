namespace ClaudeCodeUsage.Core.Models;

/// <summary>
/// User configuration, persisted as JSON to
/// <c>%APPDATA%\ClaudeCodeUsage\config.json</c>.
/// </summary>
public sealed class AppConfig
{
    /// <summary>
    /// Fetch official percentages from Anthropic's usage endpoint (the numbers
    /// Claude Code's <c>/usage</c> shows), using the OAuth token in
    /// <c>.claude\.credentials.json</c>. Turn this off to stay fully offline and use
    /// the local-log estimate only.
    /// </summary>
    public bool UseOfficialUsage { get; set; } = true;

    /// <summary>
    /// Per-window token budget for the LOCAL-LOG FALLBACK only — ignored while the
    /// official endpoint is answering. It is a placeholder: the real budget depends on
    /// your plan and on how cache-read tokens are counted, so calibrate it in Settings
    /// by watching your real peak usage.
    /// </summary>
    public long TokenLimit { get; set; } = 20_000_000;

    /// <summary>Length of the rolling window in hours. Anthropic's window is 5.</summary>
    public double WindowHours { get; set; } = 5.0;

    /// <summary>
    /// Root Claude directory. Logs are read from <c>{ClaudeDir}\projects\**\*.jsonl</c>.
    /// Empty means "use the default" (<c>%USERPROFILE%\.claude</c>).
    /// </summary>
    public string ClaudeDir { get; set; } = "";

    /// <summary>Remembered widget position (device-independent pixels). NaN = unset.</summary>
    public double WidgetLeft { get; set; } = double.NaN;
    public double WidgetTop { get; set; } = double.NaN;

    /// <summary>Whether the widget was visible when the app last closed.</summary>
    public bool WidgetVisible { get; set; } = true;

    /// <summary>Whether "Start with Windows" is enabled (mirrors the registry state).</summary>
    public bool StartWithWindows { get; set; }
}
