using ClaudeCodeUsage.Core;

namespace ClaudeCodeUsage.Tests;

public class OAuthUsageClientTests
{
    /// <summary>Trimmed from a real /api/oauth/usage response, keeping the fields we read
    /// plus enough neighbours (nulls, unknown keys) to prove they are ignored safely.</summary>
    private const string RealShape = """
        {
          "five_hour": {
            "utilization": 27.0,
            "resets_at": "2026-09-16T19:50:00.198829+00:00",
            "limit_dollars": null
          },
          "seven_day": {
            "utilization": 48.0,
            "resets_at": "2026-09-20T21:59:59.198853+00:00",
            "locked_reason": null
          },
          "seven_day_opus": null,
          "nimbus_quill": { "utilization": 0.0, "resets_at": null },
          "extra_usage": { "is_enabled": false, "used_credits": 0.0 }
        }
        """;

    [Fact]
    public void Parse_reads_both_windows_as_fractions()
    {
        var usage = OAuthUsageClient.Parse(RealShape);

        Assert.NotNull(usage);
        Assert.Equal(0.27, usage!.Session!.Utilization, 5);
        Assert.Equal(0.48, usage.Weekly!.Utilization, 5);
    }

    [Fact]
    public void Parse_normalizes_reset_times_to_utc()
    {
        var usage = OAuthUsageClient.Parse(RealShape);

        Assert.Equal(new DateTime(2026, 9, 16, 19, 50, 0, DateTimeKind.Utc),
            usage!.Session!.ResetUtc!.Value, TimeSpan.FromSeconds(1));
        Assert.Equal(DateTimeKind.Utc, usage.Weekly!.ResetUtc!.Value.Kind);
    }

    [Fact]
    public void Parse_returns_session_only_when_weekly_is_absent()
    {
        var usage = OAuthUsageClient.Parse("""{ "five_hour": { "utilization": 5.0 } }""");

        Assert.NotNull(usage);
        Assert.Equal(0.05, usage!.Session!.Utilization, 5);
        Assert.Null(usage.Weekly);
        Assert.Null(usage.Session.ResetUtc);
    }

    [Theory]
    [InlineData("not json at all")]
    [InlineData("[]")]
    [InlineData("""{ "five_hour": null, "seven_day": null }""")]      // both windows nulled out
    [InlineData("""{ "renamed_window": { "utilization": 42.0 } }""")] // endpoint shape changed
    [InlineData("""{ "five_hour": { "resets_at": "2026-09-16T19:50:00Z" } }""")] // no utilization
    public void Parse_returns_null_rather_than_a_misleading_zero(string json)
    {
        Assert.Null(OAuthUsageClient.Parse(json));
    }

    [Fact]
    public void ReadAccessToken_returns_null_when_credentials_are_missing()
    {
        var dir = Path.Combine(Path.GetTempPath(), "cc-usage-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(dir);
        try
        {
            Assert.Null(OAuthUsageClient.ReadAccessToken(dir));

            // Present but not the shape we expect — still null, never a throw.
            File.WriteAllText(OAuthUsageClient.CredentialsPath(dir), """{ "somethingElse": 1 }""");
            Assert.Null(OAuthUsageClient.ReadAccessToken(dir));

            File.WriteAllText(OAuthUsageClient.CredentialsPath(dir), "{ corrupt");
            Assert.Null(OAuthUsageClient.ReadAccessToken(dir));
        }
        finally
        {
            Directory.Delete(dir, recursive: true);
        }
    }

    [Fact]
    public void ReadAccessToken_reads_the_claude_code_credential_shape()
    {
        var dir = Path.Combine(Path.GetTempPath(), "cc-usage-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(dir);
        try
        {
            File.WriteAllText(OAuthUsageClient.CredentialsPath(dir),
                """{ "claudeAiOauth": { "accessToken": "tok-123", "expiresAt": 1789570200 } }""");

            Assert.Equal("tok-123", OAuthUsageClient.ReadAccessToken(dir));
        }
        finally
        {
            Directory.Delete(dir, recursive: true);
        }
    }
}
