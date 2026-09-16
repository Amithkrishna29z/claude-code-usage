using System.Text.Json;
using System.Text.Json.Serialization;
using ClaudeCodeUsage.Core.Models;

namespace ClaudeCodeUsage.Core;

/// <summary>Loads and saves <see cref="AppConfig"/> as JSON under
/// <c>%APPDATA%\ClaudeCodeUsage\config.json</c>. Never throws on read: a missing or
/// corrupt file yields defaults.</summary>
public sealed class ConfigService
{
    private static readonly JsonSerializerOptions Options = new()
    {
        WriteIndented = true,
        // Widget position defaults to NaN ("unset") until the window is first placed.
        NumberHandling = JsonNumberHandling.AllowNamedFloatingPointLiterals,
    };

    public string ConfigDir { get; }
    public string ConfigPath { get; }

    /// <param name="configDir">Override for the config directory (used by tests).
    /// Defaults to <c>%APPDATA%\ClaudeCodeUsage</c>.</param>
    public ConfigService(string? configDir = null)
    {
        ConfigDir = configDir ?? Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData),
            "ClaudeCodeUsage");
        ConfigPath = Path.Combine(ConfigDir, "config.json");
    }

    public AppConfig Load()
    {
        try
        {
            if (!File.Exists(ConfigPath)) return new AppConfig();
            var json = File.ReadAllText(ConfigPath);
            return JsonSerializer.Deserialize<AppConfig>(json, Options) ?? new AppConfig();
        }
        catch
        {
            return new AppConfig();
        }
    }

    public void Save(AppConfig config)
    {
        Directory.CreateDirectory(ConfigDir);
        var json = JsonSerializer.Serialize(config, Options);
        File.WriteAllText(ConfigPath, json);
    }
}
