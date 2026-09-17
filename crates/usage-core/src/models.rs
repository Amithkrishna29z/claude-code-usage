//! Data types shared by every stage of the pipeline.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

/// One usage-bearing event parsed from a Claude Code session log line
/// (a JSON object that contains a `message.usage` block).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageEvent {
    /// Top-level line `timestamp` (ISO-8601), normalized to UTC.
    pub timestamp: DateTime<Utc>,
    /// The assistant message id (`message.id`), used to de-duplicate the same event
    /// when it appears in more than one file (e.g. resumed sessions).
    pub message_id: Option<String>,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_creation_tokens: i64,
    pub cache_read_tokens: i64,
}

impl UsageEvent {
    /// Total tokens attributed to this event: input + output + cache-creation +
    /// cache-read. Cache-read usually dominates. This single definition is used
    /// everywhere; see the README for how to change it.
    pub fn total_tokens(&self) -> i64 {
        self.input_tokens + self.output_tokens + self.cache_creation_tokens + self.cache_read_tokens
    }
}

/// One rate-limit window (the 5-hour session window, or the 7-day weekly window) as a
/// fraction consumed plus when it resets. Both the official endpoint and the local-log
/// fallback produce these, so the UI renders one shape regardless of source.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UsageWindow {
    /// Fraction of the window consumed (0..1+, can exceed 1).
    pub utilization: f64,
    /// When this window resets, if known.
    pub reset_at: Option<DateTime<Utc>>,
}

impl UsageWindow {
    pub fn new(utilization: f64, reset_at: Option<DateTime<Utc>>) -> Self {
        Self {
            utilization,
            reset_at,
        }
    }

    /// Time remaining until reset. `None` when unknown; never negative.
    pub fn time_until_reset(&self, now: DateTime<Utc>) -> Option<Duration> {
        self.reset_at.map(|at| {
            let remaining = at - now;
            if remaining < Duration::zero() {
                Duration::zero()
            } else {
                remaining
            }
        })
    }
}

/// Overall health of the most recent snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageState {
    /// Usable numbers are present for the session window.
    Ok,
    /// Logs were found and parsed, but the last window has fully elapsed with no
    /// recent activity — usage has effectively reset to zero.
    NoActiveSession,
    /// Logs were found but contained no parseable usage events.
    NoData,
    /// The Claude logs directory does not exist / has no `*.jsonl` files.
    NoLogsFound,
}

/// Where a snapshot's numbers came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageSource {
    /// Anthropic's own utilization endpoint — the same numbers `/usage` shows.
    Official,
    /// Derived locally from Claude Code session logs against a configured token limit.
    LocalLogs,
}

/// Immutable, UI-ready summary of Claude Code usage. `session` and `weekly` are the
/// display surface; the token fields are only meaningful when `source` is `LocalLogs`.
#[derive(Debug, Clone)]
pub struct UsageSnapshot {
    pub state: UsageState,
    pub source: UsageSource,
    pub session: Option<UsageWindow>,
    pub weekly: Option<UsageWindow>,
    pub tokens_used: i64,
    pub token_limit: i64,
    pub window_start: Option<DateTime<Utc>>,
    /// Timestamp of the most recent local usage event. Drives the freshness line and
    /// stale detection, and is read from the logs even when the official source
    /// supplies the percentages.
    pub last_activity: Option<DateTime<Utc>>,
    pub generated_at: DateTime<Utc>,
    pub events_counted: usize,
}

impl UsageSnapshot {
    pub fn empty(state: UsageState, token_limit: i64, now: DateTime<Utc>) -> Self {
        Self {
            state,
            source: UsageSource::LocalLogs,
            session: None,
            weekly: None,
            tokens_used: 0,
            token_limit,
            window_start: None,
            last_activity: None,
            generated_at: now,
            events_counted: 0,
        }
    }

    /// Fraction of the session window consumed (0..1+). 0 when unknown.
    pub fn percent_used(&self) -> f64 {
        self.session.map(|w| w.utilization).unwrap_or(0.0)
    }

    /// When the session window resets, if known.
    pub fn reset_at(&self) -> Option<DateTime<Utc>> {
        self.session.and_then(|w| w.reset_at)
    }

    /// Time remaining until the session window resets. `None` when unknown.
    pub fn time_until_reset(&self, now: DateTime<Utc>) -> Option<Duration> {
        self.session.and_then(|w| w.time_until_reset(now))
    }
}

/// Session and weekly windows as returned by the official usage endpoint.
#[derive(Debug, Clone, PartialEq)]
pub struct OfficialUsage {
    pub session: Option<UsageWindow>,
    pub weekly: Option<UsageWindow>,
}

fn default_true() -> bool {
    true
}

fn default_token_limit() -> i64 {
    20_000_000
}

fn default_window_hours() -> f64 {
    5.0
}

fn default_refresh_seconds() -> u64 {
    300
}

/// User configuration, persisted as JSON in the platform config directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    /// Fetch official percentages from Anthropic's usage endpoint, using the OAuth
    /// token in `.claude/.credentials.json`. Turn this off to stay fully offline and
    /// use the local-log estimate only.
    #[serde(default = "default_true")]
    pub use_official_usage: bool,

    /// Per-window token budget for the LOCAL-LOG FALLBACK only — ignored while the
    /// official endpoint is answering. It is a placeholder: the real budget depends on
    /// your plan and on how cache-read tokens are counted.
    #[serde(default = "default_token_limit")]
    pub token_limit: i64,

    /// Length of the rolling window in hours. Anthropic's window is 5.
    #[serde(default = "default_window_hours")]
    pub window_hours: f64,

    /// How often to re-fetch the official figures, in seconds.
    ///
    /// Clamped to a 60-second floor at use: the endpoint answers `429` with a
    /// `Retry-After` of a few minutes, and polling harder just gets the app banned
    /// into its local-estimate fallback, which is strictly worse than a slightly
    /// stale official number. The displayed countdown ticks every second regardless
    /// of this value.
    #[serde(default = "default_refresh_seconds")]
    pub refresh_seconds: u64,

    /// Root Claude directory. Logs are read from `{claude_dir}/projects/**/*.jsonl`.
    /// Empty means "use the default" (`~/.claude`).
    pub claude_dir: String,

    /// Remembered widget position. `None` = unset.
    pub widget_left: Option<f32>,
    pub widget_top: Option<f32>,

    /// Whether the widget was visible when the app last closed.
    #[serde(default = "default_true")]
    pub widget_visible: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            use_official_usage: true,
            token_limit: default_token_limit(),
            window_hours: default_window_hours(),
            refresh_seconds: default_refresh_seconds(),
            claude_dir: String::new(),
            widget_left: None,
            widget_top: None,
            widget_visible: true,
        }
    }
}
