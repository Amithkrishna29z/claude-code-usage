using System.Globalization;
using System.Text.Json;
using ClaudeCodeUsage.Core.Models;

namespace ClaudeCodeUsage.Core;

/// <summary>
/// Pure, side-effect-free parsing of Claude Code session-log JSONL lines into
/// <see cref="UsageEvent"/>s. Every line is parsed defensively: a malformed line,
/// a missing <c>message.usage</c>, or a missing/renamed field never throws — it is
/// skipped (with an optional warning) so the caller keeps running.
///
/// Verified against real logs (Claude Code v2.1.x), where each usage-bearing line
/// looks like:
///   {"type":"assistant","timestamp":"2026-07-27T15:22:21.625Z",
///    "message":{"id":"...","usage":{"input_tokens":..,"output_tokens":..,
///               "cache_creation_input_tokens":..,"cache_read_input_tokens":..}}}
/// See README ("Re-verifying the log format") if these paths ever change.
/// </summary>
public static class UsageParser
{
    /// <summary>Parses a sequence of raw JSONL lines, yielding one event per line
    /// that carries a usage block. Blank lines and non-usage lines are ignored.</summary>
    /// <param name="lines">Raw text lines (each expected to be one JSON object).</param>
    /// <param name="onWarn">Optional sink for diagnostic messages (bad line, etc.).</param>
    public static IEnumerable<UsageEvent> ParseLines(
        IEnumerable<string> lines, Action<string>? onWarn = null)
    {
        foreach (var line in lines)
        {
            if (string.IsNullOrWhiteSpace(line)) continue;

            UsageEvent? evt;
            try
            {
                evt = ParseLine(line);
            }
            catch (Exception ex)
            {
                onWarn?.Invoke($"Skipped malformed log line: {ex.Message}");
                continue;
            }

            if (evt is not null) yield return evt;
        }
    }

    /// <summary>Parses a single JSONL line. Returns null when the line carries no
    /// usage block or lacks a usable timestamp. Throws only on invalid JSON, which
    /// <see cref="ParseLines"/> catches.</summary>
    public static UsageEvent? ParseLine(string line)
    {
        using var doc = JsonDocument.Parse(line);
        var root = doc.RootElement;
        if (root.ValueKind != JsonValueKind.Object) return null;

        if (!root.TryGetProperty("message", out var message) ||
            message.ValueKind != JsonValueKind.Object)
            return null;

        if (!message.TryGetProperty("usage", out var usage) ||
            usage.ValueKind != JsonValueKind.Object)
            return null;

        // Timestamp lives at the top level of the line object.
        if (!TryReadTimestamp(root, out var timestampUtc))
            return null;

        return new UsageEvent
        {
            TimestampUtc = timestampUtc,
            MessageId = TryReadString(message, "id"),
            InputTokens = ReadLong(usage, "input_tokens"),
            OutputTokens = ReadLong(usage, "output_tokens"),
            CacheCreationTokens = ReadLong(usage, "cache_creation_input_tokens"),
            CacheReadTokens = ReadLong(usage, "cache_read_input_tokens"),
        };
    }

    private static bool TryReadTimestamp(JsonElement obj, out DateTime utc)
    {
        utc = default;
        if (!obj.TryGetProperty("timestamp", out var ts) ||
            ts.ValueKind != JsonValueKind.String)
            return false;

        var raw = ts.GetString();
        if (string.IsNullOrEmpty(raw)) return false;

        if (!DateTimeOffset.TryParse(raw, CultureInfo.InvariantCulture,
                DateTimeStyles.AdjustToUniversal | DateTimeStyles.AssumeUniversal,
                out var dto))
            return false;

        utc = dto.UtcDateTime;
        return true;
    }

    /// <summary>Reads an integer token field, tolerating absence, nulls, and
    /// numbers-as-strings. Missing/unparseable fields count as 0.</summary>
    private static long ReadLong(JsonElement obj, string name)
    {
        if (!obj.TryGetProperty(name, out var el)) return 0;
        return el.ValueKind switch
        {
            JsonValueKind.Number => el.TryGetInt64(out var n) ? n : 0,
            JsonValueKind.String => long.TryParse(el.GetString(), NumberStyles.Integer,
                CultureInfo.InvariantCulture, out var s) ? s : 0,
            _ => 0,
        };
    }

    private static string? TryReadString(JsonElement obj, string name) =>
        obj.TryGetProperty(name, out var el) && el.ValueKind == JsonValueKind.String
            ? el.GetString()
            : null;
}
