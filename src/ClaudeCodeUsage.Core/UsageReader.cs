using ClaudeCodeUsage.Core.Models;

namespace ClaudeCodeUsage.Core;

/// <summary>
/// Reads and de-duplicates usage events from all Claude Code session logs under
/// <c>{claudeDir}\projects\**\*.jsonl</c>. This is the only file-touching part of
/// the core library; it uses read-only, shared-access streaming so it never blocks
/// Claude Code from writing.
/// </summary>
public static class UsageReader
{
    /// <summary>Resolves the effective Claude root directory. Empty/whitespace
    /// falls back to <c>%USERPROFILE%\.claude</c>.</summary>
    public static string ResolveClaudeDir(string? configuredDir)
    {
        if (!string.IsNullOrWhiteSpace(configuredDir))
            return Environment.ExpandEnvironmentVariables(configuredDir);

        var home = Environment.GetFolderPath(Environment.SpecialFolder.UserProfile);
        return Path.Combine(home, ".claude");
    }

    /// <summary>The directory that holds per-project session logs.</summary>
    public static string ProjectsDir(string claudeDir) => Path.Combine(claudeDir, "projects");

    public sealed record ReadResult(IReadOnlyList<UsageEvent> Events, bool LogsFound);

    /// <summary>
    /// Reads every *.jsonl under the projects directory, parses usage events, and
    /// de-duplicates by message id (the same assistant message can appear in more
    /// than one file after a session resume). Returns events plus whether any log
    /// files were found at all.
    /// </summary>
    public static ReadResult ReadEvents(string claudeDir, Action<string>? onWarn = null)
    {
        var projects = ProjectsDir(claudeDir);
        if (!Directory.Exists(projects))
            return new ReadResult(Array.Empty<UsageEvent>(), LogsFound: false);

        string[] files;
        try
        {
            files = Directory.GetFiles(projects, "*.jsonl", SearchOption.AllDirectories);
        }
        catch (Exception ex)
        {
            onWarn?.Invoke($"Could not enumerate logs: {ex.Message}");
            return new ReadResult(Array.Empty<UsageEvent>(), LogsFound: false);
        }

        if (files.Length == 0)
            return new ReadResult(Array.Empty<UsageEvent>(), LogsFound: false);

        var events = new List<UsageEvent>();
        var seenIds = new HashSet<string>(StringComparer.Ordinal);

        foreach (var file in files)
        {
            foreach (var evt in UsageParser.ParseLines(ReadLinesSafe(file, onWarn), onWarn))
            {
                if (evt.MessageId is { Length: > 0 } id && !seenIds.Add(id))
                    continue; // duplicate of an already-counted message
                events.Add(evt);
            }
        }

        return new ReadResult(events, LogsFound: true);
    }

    /// <summary>Streams lines from a log file with shared read/write access so an
    /// actively-written log is still readable. Any per-file IO error is reported
    /// and that file is skipped.</summary>
    private static IEnumerable<string> ReadLinesSafe(string path, Action<string>? onWarn)
    {
        StreamReader reader;
        try
        {
            var stream = new FileStream(path, FileMode.Open, FileAccess.Read,
                FileShare.ReadWrite | FileShare.Delete);
            reader = new StreamReader(stream);
        }
        catch (Exception ex)
        {
            onWarn?.Invoke($"Could not open {Path.GetFileName(path)}: {ex.Message}");
            yield break;
        }

        using (reader)
        {
            string? line;
            while ((line = reader.ReadLine()) is not null)
                yield return line;
        }
    }
}
