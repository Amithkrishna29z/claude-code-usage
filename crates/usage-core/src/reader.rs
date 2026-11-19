//! Reads and de-duplicates usage events from all Claude Code session logs under
//! `{claude_dir}/projects/**/*.jsonl`. Files are streamed line by line and opened
//! read-only, so an actively-written log never blocks Claude Code.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
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

/// Per-file state kept across polls so an actively-growing session log is read
/// incrementally — only the bytes written since the last poll — instead of being
/// reopened and reparsed from byte zero every time. A session log can run to several
/// megabytes over a few hours; without this, that whole file is read and reparsed on
/// every poll for as long as the session stays open.
///
/// The caller owns one of these for the life of the process and passes it to every
/// [`read_events`] call. A file that falls out of the requested window (or disappears)
/// has its entry dropped on the same call, so this does not grow for as long as the
/// window does.
#[derive(Default)]
pub struct LogCache {
    files: HashMap<PathBuf, FileState>,
}

#[derive(Default)]
struct FileState {
    /// Byte offset up to the last complete line consumed.
    offset: u64,
    /// Every event parsed from this file so far.
    events: Vec<UsageEvent>,
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
///
/// `cache` carries each touched file's read offset and already-parsed events across
/// calls, so a file that has not grown since the last call costs one stat and nothing
/// else, and one that has only costs reading and parsing the bytes appended since.
pub fn read_events(
    claude_dir: &Path,
    since: DateTime<Utc>,
    cache: &mut LogCache,
    mut on_warn: impl FnMut(String),
) -> ReadResult {
    let projects = projects_dir(claude_dir);
    let mut files = Vec::new();
    collect_jsonl(&projects, &mut files, &mut on_warn);

    if files.is_empty() {
        cache.files.clear();
        return ReadResult {
            events: Vec::new(),
            logs_found: false,
        };
    }

    let mut events = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut live = HashSet::with_capacity(files.len());

    for file in &files {
        if !touched_since(file, since) {
            continue;
        }
        live.insert(file.clone());
        let pending = ingest_new_lines(file, cache, &mut on_warn);

        let Some(state) = cache.files.get(file) else {
            continue;
        };
        for event in state.events.iter().chain(pending.iter()) {
            if let Some(id) = &event.message_id {
                if !seen.insert(id.clone()) {
                    continue; // duplicate of an already-counted message
                }
            }
            events.push(event.clone());
        }
    }

    // Drop state for files that fell out of the window or disappeared, so the cache
    // does not grow for as long as the process runs.
    cache.files.retain(|path, _| live.contains(path));

    ReadResult {
        events,
        logs_found: true,
    }
}

/// Reads whatever has been appended to `path` since `cache` last looked at it.
///
/// Lines terminated by a newline are parsed once and merged permanently into the
/// cached list. Anything after the last newline — a line still being written — is
/// never committed: it is returned instead, to be parsed fresh on every call until it
/// is eventually closed off by a newline, exactly like the rest of the file used to
/// be read every time. That keeps a slow write from ever losing data, at the cost of
/// repeating only the few open bytes rather than the whole file.
fn ingest_new_lines(
    path: &Path,
    cache: &mut LogCache,
    on_warn: &mut impl FnMut(String),
) -> Vec<UsageEvent> {
    let size = match path.metadata() {
        Ok(meta) => meta.len(),
        Err(err) => {
            on_warn(format!("Could not stat {}: {err}", path.display()));
            return Vec::new();
        }
    };

    let state = cache.files.entry(path.to_path_buf()).or_default();

    // A file smaller than what was already read was truncated or replaced from
    // scratch (log rotation, a cleared session) — the old offset no longer lines up
    // with anything in it, so start over.
    if size < state.offset {
        state.offset = 0;
        state.events.clear();
    }

    if size == state.offset {
        return Vec::new(); // nothing new since last time
    }

    let mut handle = match File::open(path) {
        Ok(h) => h,
        Err(err) => {
            on_warn(format!("Could not open {}: {err}", path.display()));
            return Vec::new();
        }
    };
    if let Err(err) = handle.seek(SeekFrom::Start(state.offset)) {
        on_warn(format!("Could not seek {}: {err}", path.display()));
        return Vec::new();
    }

    let mut buf = Vec::new();
    if let Err(err) = handle.read_to_end(&mut buf) {
        on_warn(format!("Could not read {}: {err}", path.display()));
        return Vec::new();
    }

    // Lines that are not valid UTF-8 are skipped rather than aborting the file.
    let tail = match buf.iter().rposition(|&b| b == b'\n') {
        Some(last_newline) => {
            let text = String::from_utf8_lossy(&buf[..=last_newline]);
            state.events.extend(parser::parse_lines(text.lines()));
            state.offset += last_newline as u64 + 1;
            &buf[last_newline + 1..]
        }
        None => &buf[..],
    };

    if tail.is_empty() {
        return Vec::new();
    }
    parser::parse_lines(String::from_utf8_lossy(tail).lines()).collect()
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
