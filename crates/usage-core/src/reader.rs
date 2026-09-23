//! Reads and de-duplicates usage events from all Claude Code session logs under
//! `{claude_dir}/projects/**/*.jsonl`. Files are streamed line by line and opened
//! read-only, so an actively-written log never blocks Claude Code.

use std::collections::HashSet;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};

use crate::models::UsageEvent;
use crate::parser;

pub struct ReadResult {
    pub events: Vec<UsageEvent>,
    /// Whether any log files were found at all — distinguishes "no logs" from
    /// "logs present but idle".
    pub logs_found: bool,
}

/// Resolves the effective Claude root directory. Empty/whitespace falls back to
/// `~/.claude`, which is where Claude Code puts it on every platform.
pub fn resolve_claude_dir(configured: &str) -> PathBuf {
    let trimmed = configured.trim();
    if !trimmed.is_empty() {
        return PathBuf::from(trimmed);
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".claude")
}

/// The directory that holds per-project session logs.
pub fn projects_dir(claude_dir: &Path) -> PathBuf {
    claude_dir.join("projects")
}

/// Reads every `*.jsonl` under the projects directory that can still hold a relevant
/// event, parses usage events, and de-duplicates by message id (the same assistant
/// message can appear in more than one file after a session resume).
///
/// `since` is the earliest instant worth reading — the start of the oldest session
/// window that could still be open. Logs accumulate for as long as Claude Code has
/// been used, and re-parsing all of them every poll costs far more than the answer is
/// worth: only the block covering `now` is ever reported, and every event in it is
/// newer than `since` by construction. A log not written to since then cannot contain
/// one, so it is skipped without being opened. Discovery still walks the whole tree,
/// so `logs_found` means what it always did.
pub fn read_events(
    claude_dir: &Path,
    since: DateTime<Utc>,
    mut on_warn: impl FnMut(String),
) -> ReadResult {
    let projects = projects_dir(claude_dir);
    let mut files = Vec::new();
    collect_jsonl(&projects, &mut files, &mut on_warn);

    if files.is_empty() {
        return ReadResult {
            events: Vec::new(),
            logs_found: false,
        };
    }

    let mut events = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for file in files {
        if !touched_since(&file, since) {
            continue;
        }

        let handle = match File::open(&file) {
            Ok(h) => h,
            Err(err) => {
                on_warn(format!("Could not open {}: {err}", file.display()));
                continue;
            }
        };

        // Lines that are not valid UTF-8 are skipped rather than aborting the file.
        let lines = BufReader::new(handle).lines().map_while(Result::ok);
        for event in parser::parse_lines(lines) {
            if let Some(id) = &event.message_id {
                if !seen.insert(id.clone()) {
                    continue; // duplicate of an already-counted message
                }
            }
            events.push(event);
        }
    }

    ReadResult {
        events,
        logs_found: true,
    }
}

/// Whether a log was written to at or after `cutoff`. A log whose age cannot be read
/// is read rather than skipped: guessing it stale would silently drop usage.
fn touched_since(path: &Path, cutoff: DateTime<Utc>) -> bool {
    match path.metadata().and_then(|meta| meta.modified()) {
        Ok(modified) => DateTime::<Utc>::from(modified) >= cutoff,
        Err(_) => true,
    }
}

/// Walks the project tree collecting `*.jsonl` paths. Unreadable subdirectories are
/// reported and skipped so one bad directory cannot blank the whole reading.
fn collect_jsonl(dir: &Path, out: &mut Vec<PathBuf>, on_warn: &mut impl FnMut(String)) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(err) => {
            // A missing projects dir is the normal "not set up yet" case, not a fault.
            if err.kind() != std::io::ErrorKind::NotFound {
                on_warn(format!("Could not read {}: {err}", dir.display()));
            }
            return;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_jsonl(&path, out, on_warn);
        } else if path.extension().is_some_and(|ext| ext == "jsonl") {
            out.push(path);
        }
    }
}
