using System.Net;
using System.Net.Http;
using System.Net.Http.Headers;
using System.Text.Json;
using ClaudeCodeUsage.Core.Models;

namespace ClaudeCodeUsage.Core;

/// <summary>
/// Fetches Anthropic's own utilization figures — the same numbers Claude Code's
/// <c>/usage</c> screen shows — from <c>GET /api/oauth/usage</c>, authenticated with the
/// OAuth access token Claude Code already stores in <c>{claudeDir}\.credentials.json</c>.
///
/// This endpoint is internal and undocumented: it can change or disappear without
/// notice. Every failure path here is non-fatal and returns null so the caller can fall
/// back to the local-log estimate. The token is only ever sent to api.anthropic.com.
/// </summary>
public sealed class OAuthUsageClient : IDisposable
{
    public const string DefaultEndpoint = "https://api.anthropic.com/api/oauth/usage";

    /// <summary>Applied when a 429 arrives without a usable <c>Retry-After</c>.</summary>
    private static readonly TimeSpan DefaultBackoff = TimeSpan.FromMinutes(5);

    private readonly HttpClient _http;
    private readonly string _endpoint;
    private DateTime _nextAttemptUtc = DateTime.MinValue;

    /// <summary>
    /// Earliest time a request may be sent again. The endpoint rate-limits hard (it
    /// answers 429 with a <c>Retry-After</c> of a few minutes), so a refused request is
    /// remembered and no further call goes out until it expires.
    /// </summary>
    public DateTime NextAttemptUtc => _nextAttemptUtc;

    /// <param name="endpoint">Override the URL (used by tests).</param>
    /// <param name="handler">Override the transport (used by tests).</param>
    public OAuthUsageClient(string? endpoint = null, HttpMessageHandler? handler = null)
    {
        _endpoint = string.IsNullOrWhiteSpace(endpoint) ? DefaultEndpoint : endpoint;
        _http = handler is null ? new HttpClient() : new HttpClient(handler);
        _http.Timeout = TimeSpan.FromSeconds(10);
    }

    public static string CredentialsPath(string claudeDir) =>
        Path.Combine(claudeDir, ".credentials.json");

    /// <summary>Reads the OAuth access token, or null when absent/unreadable/malformed.</summary>
    public static string? ReadAccessToken(string claudeDir)
    {
        var path = CredentialsPath(claudeDir);
        try
        {
            if (!File.Exists(path)) return null;
            using var doc = JsonDocument.Parse(File.ReadAllText(path));
            if (!doc.RootElement.TryGetProperty("claudeAiOauth", out var oauth)) return null;
            if (!oauth.TryGetProperty("accessToken", out var token)) return null;
            var value = token.GetString();
            return string.IsNullOrWhiteSpace(value) ? null : value;
        }
        catch
        {
            return null;
        }
    }

    /// <summary>
    /// Fetches the session and weekly windows. Returns null on any failure — no
    /// credentials, expired token, network error, or an unrecognised response body.
    /// </summary>
    public async Task<OfficialUsage?> FetchAsync(
        string claudeDir, Action<string>? onWarn = null, CancellationToken ct = default)
    {
        if (DateTime.UtcNow < _nextAttemptUtc) return null;

        var token = ReadAccessToken(claudeDir);
        if (token is null)
        {
            onWarn?.Invoke("No OAuth credentials found — using local log estimate.");
            return null;
        }

        try
        {
            using var request = new HttpRequestMessage(HttpMethod.Get, _endpoint);
            request.Headers.Authorization = new AuthenticationHeaderValue("Bearer", token);
            request.Headers.Add("anthropic-beta", "oauth-2025-04-20");

            using var response = await _http.SendAsync(request, ct).ConfigureAwait(false);

            if (response.StatusCode == HttpStatusCode.TooManyRequests)
            {
                var wait = response.Headers.RetryAfter?.Delta
                           ?? (response.Headers.RetryAfter?.Date is { } at
                               ? at - DateTimeOffset.UtcNow
                               : DefaultBackoff);
                if (wait < TimeSpan.Zero) wait = DefaultBackoff;
                _nextAttemptUtc = DateTime.UtcNow + wait;
                onWarn?.Invoke($"Usage endpoint rate-limited; retrying in {(int)wait.TotalSeconds}s.");
                return null;
            }

            if (!response.IsSuccessStatusCode)
            {
                onWarn?.Invoke(response.StatusCode == HttpStatusCode.Unauthorized
                    ? "Usage endpoint rejected the token (re-run `claude` to refresh it)."
                    : $"Usage endpoint returned {(int)response.StatusCode}.");
                _nextAttemptUtc = DateTime.UtcNow + DefaultBackoff;
                return null;
            }

            var json = await response.Content.ReadAsStringAsync(ct).ConfigureAwait(false);
            return Parse(json, onWarn);
        }
        catch (Exception ex)
        {
            onWarn?.Invoke($"Usage endpoint unreachable: {ex.Message}");
            return null;
        }
    }

    /// <summary>
    /// Parses the endpoint body. Shape (only the fields we use):
    /// <code>
    /// { "five_hour": { "utilization": 27.0, "resets_at": "2026-09-16T19:50:00+00:00" },
    ///   "seven_day": { "utilization": 48.0, "resets_at": "2026-09-20T21:59:59+00:00" } }
    /// </code>
    /// Every field is read defensively: anything missing or renamed yields null for that
    /// window rather than an exception. Returns null when neither window is present, so
    /// a shape change falls back to the local estimate instead of showing 0%.
    /// </summary>
    public static OfficialUsage? Parse(string json, Action<string>? onWarn = null)
    {
        try
        {
            using var doc = JsonDocument.Parse(json);
            var root = doc.RootElement;
            if (root.ValueKind != JsonValueKind.Object) return null;

            var session = ReadWindow(root, "five_hour");
            var weekly = ReadWindow(root, "seven_day");
            if (session is null && weekly is null)
            {
                onWarn?.Invoke("Usage endpoint returned no recognised windows.");
                return null;
            }

            return new OfficialUsage(session, weekly);
        }
        catch (Exception ex)
        {
            onWarn?.Invoke($"Could not parse usage response: {ex.Message}");
            return null;
        }
    }

    /// <summary>The endpoint reports utilization as a percentage (27.0 == 27%).</summary>
    private static UsageWindow? ReadWindow(JsonElement root, string name)
    {
        if (!root.TryGetProperty(name, out var node) || node.ValueKind != JsonValueKind.Object)
            return null;

        if (!node.TryGetProperty("utilization", out var util)
            || !util.TryGetDouble(out var percent))
            return null;

        DateTime? resetUtc = null;
        if (node.TryGetProperty("resets_at", out var resets)
            && resets.ValueKind == JsonValueKind.String
            && DateTimeOffset.TryParse(resets.GetString(), out var parsed))
            resetUtc = parsed.UtcDateTime;

        return new UsageWindow { Utilization = percent / 100.0, ResetUtc = resetUtc };
    }

    public void Dispose() => _http.Dispose();
}
