//! JSONL line -> [`UsageEvent`]. Every field is read defensively: a missing or
//! renamed field is treated as `0`/skipped and never panics, so a log-format change
//! degrades the estimate instead of crashing the app.

use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::models::UsageEvent;

/// Parses one log line. Returns `None` for blank lines, malformed JSON, and lines
/// that carry no `message.usage` block (user turns, metadata, and so on).
pub fn parse_line(line: &str) -> Option<UsageEvent> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }

    let root: Value = serde_json::from_str(line).ok()?;
    let usage = root.get("message")?.get("usage")?;
    if !usage.is_object() {
        return None;
    }

    let timestamp = parse_timestamp(root.get("timestamp")?)?;

    let message_id = root
        .get("message")
        .and_then(|m| m.get("id"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_owned);

    Some(UsageEvent {
        timestamp,
        message_id,
        input_tokens: token_field(usage, "input_tokens"),
        output_tokens: token_field(usage, "output_tokens"),
        cache_creation_tokens: token_field(usage, "cache_creation_input_tokens"),
        cache_read_tokens: token_field(usage, "cache_read_input_tokens"),
    })
}

/// Parses every usage-bearing line in an iterator of lines.
pub fn parse_lines<I, S>(lines: I) -> impl Iterator<Item = UsageEvent>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    lines
        .into_iter()
        .filter_map(|line| parse_line(line.as_ref()))
}

/// A missing, null, or non-numeric token count is worth zero, never an error.
fn token_field(usage: &Value, name: &str) -> i64 {
    usage.get(name).and_then(Value::as_i64).unwrap_or(0)
}

fn parse_timestamp(value: &Value) -> Option<DateTime<Utc>> {
    let text = value.as_str()?;
    DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}
