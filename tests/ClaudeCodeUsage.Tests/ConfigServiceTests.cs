using ClaudeCodeUsage.Core;
using ClaudeCodeUsage.Core.Models;

namespace ClaudeCodeUsage.Tests;

public class ConfigServiceTests : IDisposable
{
    private readonly string _dir;

    public ConfigServiceTests()
    {
        _dir = Path.Combine(Path.GetTempPath(), "ccu-cfg-" + Guid.NewGuid().ToString("N"));
    }

    public void Dispose()
    {
        try { Directory.Delete(_dir, recursive: true); } catch { /* best effort */ }
    }

    [Fact]
    public void Load_returns_defaults_when_file_absent()
    {
        var svc = new ConfigService(_dir);
        var cfg = svc.Load();
        Assert.Equal(5.0, cfg.WindowHours);
        Assert.True(cfg.TokenLimit > 0);
    }

    [Fact]
    public void Save_then_load_round_trips()
    {
        var svc = new ConfigService(_dir);
        svc.Save(new AppConfig { TokenLimit = 12345, WindowHours = 6, ClaudeDir = @"C:\x" });

        var cfg = new ConfigService(_dir).Load();
        Assert.Equal(12345, cfg.TokenLimit);
        Assert.Equal(6, cfg.WindowHours);
        Assert.Equal(@"C:\x", cfg.ClaudeDir);
    }

    [Fact]
    public void Load_returns_defaults_on_corrupt_file()
    {
        Directory.CreateDirectory(_dir);
        File.WriteAllText(Path.Combine(_dir, "config.json"), "{ this is not valid json");

        var cfg = new ConfigService(_dir).Load();
        Assert.Equal(5.0, cfg.WindowHours);
    }
}
