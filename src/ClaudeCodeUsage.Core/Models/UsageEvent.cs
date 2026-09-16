namespace ClaudeCodeUsage.Core.Models;

/// <summary>
/// One usage-bearing event parsed from a Claude Code session log line
/// (a JSON object that contains a <c>message.usage</c> block).
/// </summary>
public sealed class UsageEvent
{
    /// <summary>Top-level line <c>timestamp</c> (ISO-8601), normalized to UTC.</summary>
    public DateTime TimestampUtc { get; init; }

    /// <summary>The assistant message id (<c>message.id</c>), used to de-duplicate
    /// the same event when it appears in more than one file (e.g. resumed sessions).</summary>
    public string? MessageId { get; init; }

    public long InputTokens { get; init; }
    public long OutputTokens { get; init; }
    public long CacheCreationTokens { get; init; }
    public long CacheReadTokens { get; init; }

    /// <summary>
    /// Total tokens attributed to this event. Sum of all four components:
    /// input + output + cache-creation + cache-read. Cache-read usually dominates.
    /// This single definition is used everywhere; see README for how to change it.
    /// </summary>
    public long TotalTokens => InputTokens + OutputTokens + CacheCreationTokens + CacheReadTokens;
}
