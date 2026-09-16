using ClaudeCodeUsage.Core;

namespace ClaudeCodeUsage.Tests;

public class UsageParserTests
{
    private static string[] FixtureLines() =>
        File.ReadAllLines(Path.Combine(AppContext.BaseDirectory, "Fixtures", "sample-usage.jsonl"));

    [Fact]
    public void ParseLines_ignores_non_usage_and_malformed_lines()
    {
        var events = UsageParser.ParseLines(FixtureLines()).ToList();

        // 5 usage-bearing lines (m1, m2, m3, m3-dup, m4); the user line and the
        // garbage line are skipped. De-duplication happens in UsageReader, not here.
        Assert.Equal(5, events.Count);
    }

    [Fact]
    public void ParseLines_sums_all_four_token_components()
    {
        var events = UsageParser.ParseLines(FixtureLines()).ToList();

        var m2 = events.Single(e => e.TimestampUtc == DateTime.Parse("2026-08-15T10:00:00Z").ToUniversalTime());
        Assert.Equal(1000, m2.InputTokens);
        Assert.Equal(500, m2.OutputTokens);
        Assert.Equal(2000, m2.CacheCreationTokens);
        Assert.Equal(4000, m2.CacheReadTokens);
        Assert.Equal(7500, m2.TotalTokens);
    }

    [Fact]
    public void ParseLine_treats_missing_token_fields_as_zero()
    {
        // m4 has input/output but no cache fields.
        var line = FixtureLines().Single(l => l.Contains("\"m4\""));
        var evt = UsageParser.ParseLine(line);

        Assert.NotNull(evt);
        Assert.Equal(0, evt!.CacheCreationTokens);
        Assert.Equal(0, evt.CacheReadTokens);
        Assert.Equal(15, evt.TotalTokens);
    }

    [Fact]
    public void ParseLine_returns_null_when_no_usage_block()
    {
        var evt = UsageParser.ParseLine(
            "{\"type\":\"user\",\"timestamp\":\"2026-08-15T09:00:00Z\",\"message\":{\"role\":\"user\"}}");
        Assert.Null(evt);
    }

    [Fact]
    public void ParseLine_returns_null_when_timestamp_missing()
    {
        var evt = UsageParser.ParseLine(
            "{\"type\":\"assistant\",\"message\":{\"usage\":{\"input_tokens\":5}}}");
        Assert.Null(evt);
    }

    [Fact]
    public void ParseLines_does_not_throw_on_garbage()
    {
        var events = UsageParser.ParseLines(new[] { "not json", "", "   ", "{\"broken\": " }).ToList();
        Assert.Empty(events);
    }
}
