using ClaudeCodeUsage.Core;

namespace ClaudeCodeUsage.Tests;

public class UsageReaderTests : IDisposable
{
    private readonly string _root;

    public UsageReaderTests()
    {
        _root = Path.Combine(Path.GetTempPath(), "ccu-tests-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(Path.Combine(_root, "projects", "proj-a"));
        Directory.CreateDirectory(Path.Combine(_root, "projects", "proj-b"));
    }

    public void Dispose()
    {
        try { Directory.Delete(_root, recursive: true); } catch { /* best effort */ }
    }

    private static string UsageLine(string id, string ts, long input) =>
        $"{{\"type\":\"assistant\",\"timestamp\":\"{ts}\",\"message\":{{\"id\":\"{id}\"," +
        $"\"usage\":{{\"input_tokens\":{input},\"output_tokens\":0," +
        "\"cache_creation_input_tokens\":0,\"cache_read_input_tokens\":0}}}";

    [Fact]
    public void ReadEvents_reports_no_logs_when_projects_dir_missing()
    {
        var result = UsageReader.ReadEvents(Path.Combine(_root, "does-not-exist"));
        Assert.False(result.LogsFound);
        Assert.Empty(result.Events);
    }

    [Fact]
    public void ReadEvents_dedupes_same_message_id_across_files()
    {
        File.WriteAllLines(Path.Combine(_root, "projects", "proj-a", "s1.jsonl"), new[]
        {
            UsageLine("dup", "2026-08-15T10:00:00Z", 100),
            UsageLine("unique-a", "2026-08-15T10:01:00Z", 200),
        });
        // Same "dup" id appears again in another file (resumed session).
        File.WriteAllLines(Path.Combine(_root, "projects", "proj-b", "s2.jsonl"), new[]
        {
            UsageLine("dup", "2026-08-15T10:00:00Z", 100),
            UsageLine("unique-b", "2026-08-15T10:02:00Z", 300),
        });

        var result = UsageReader.ReadEvents(_root);

        Assert.True(result.LogsFound);
        Assert.Equal(3, result.Events.Count); // dup counted once
        Assert.Equal(600, result.Events.Sum(e => e.TotalTokens)); // 100 + 200 + 300
    }
}
