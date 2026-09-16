//! Fetches Anthropic's own utilization figures — the same numbers Claude Code's
//! `/usage` screen shows — from `GET /api/oauth/usage`, authenticated with the OAuth
//! access token Claude Code already stores in `{claude_dir}/.credentials.json`.
//!
//! This endpoint is internal and undocumented: it can change or disappear without
//! notice. Every failure path here is non-fatal and returns `None` so the caller can
//! fall back to the local-log estimate. The token is only ever sent to
//! `api.anthropic.com`.

use std::path::{Path, PathBuf};
use std::time::Duration as StdDuration;

use chrono::{DateTime, Duration, Utc};
use serde_json::Value;

use crate::models::{OfficialUsage, UsageWindow};

pub const DEFAULT_ENDPOINT: &str = "https://api.anthropic.com/api/oauth/usage";

/// Applied when a 429 arrives without a usable `Retry-After`.
const DEFAULT_BACKOFF_SECS: i64 = 300;

pub struct OAuthUsageClient {
    endpoint: String,
    /// Earliest time a request may be sent again. The endpoint rate-limits hard (it
    /// answers 429 with a `Retry-After` of a few minutes), so a refused request is
    /// remembered and no further call goes out until it expires.
    next_attempt: Option<DateTime<Utc>>,
}

impl Default for OAuthUsageClient {
    fn default() -> Self {
        Self::new(DEFAULT_ENDPOINT)
    }
}

impl OAuthUsageClient {
    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
            next_attempt: None,
        }
    }

    pub fn next_attempt(&self) -> Option<DateTime<Utc>> {
        self.next_attempt
    }

    /// Fetches the session and weekly windows. Returns `None` on any failure — no
    /// credentials, expired token, network error, rate limit, or an unrecognised
    /// response body.
    pub fn fetch(
        &mut self,
        claude_dir: &Path,
        now: DateTime<Utc>,
        mut on_warn: impl FnMut(String),
    ) -> Option<OfficialUsage> {
        if let Some(next) = self.next_attempt {
            if now < next {
                return None;
            }
        }

        let token = match read_access_token(claude_dir) {
            Some(t) => t,
            None => {
                on_warn("No OAuth credentials found — using local log estimate.".into());
                return None;
            }
        };

        let request = ureq::get(&self.endpoint)
            .timeout(StdDuration::from_secs(10))
            .set("Authorization", &format!("Bearer {token}"))
            .set("anthropic-beta", "oauth-2025-04-20");

        match request.call() {
            Ok(response) => {
                let body = response.into_string().ok()?;
                parse(&body, &mut on_warn)
            }
            Err(ureq::Error::Status(429, response)) => {
                let wait = response
                    .header("Retry-After")
                    .and_then(|v| v.trim().parse::<i64>().ok())
                    .filter(|secs| *secs > 0)
                    .unwrap_or(DEFAULT_BACKOFF_SECS);
                self.next_attempt = Some(now + Duration::seconds(wait));
                on_warn(format!("Usage endpoint rate-limited; retrying in {wait}s."));
                None
            }
            Err(ureq::Error::Status(401, _)) => {
                self.next_attempt = Some(now + Duration::seconds(DEFAULT_BACKOFF_SECS));
                on_warn("Usage endpoint rejected the token (run `claude` to refresh it).".into());
                None
            }
            Err(ureq::Error::Status(code, _)) => {
                self.next_attempt = Some(now + Duration::seconds(DEFAULT_BACKOFF_SECS));
                on_warn(format!("Usage endpoint returned {code}."));
                None
            }
            Err(err) => {
                on_warn(format!("Usage endpoint unreachable: {err}"));
                None
            }
        }
    }
}

pub fn credentials_path(claude_dir: &Path) -> PathBuf {
    claude_dir.join(".credentials.json")
}

/// Reads the OAuth access token, or `None` when absent/unreadable/malformed.
pub fn read_access_token(claude_dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(credentials_path(claude_dir)).ok()?;
    let root: Value = serde_json::from_str(&text).ok()?;
    root.get("claudeAiOauth")?
        .get("accessToken")?
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

/// Parses the endpoint body. Shape (only the fields we use):
///
/// ```json
/// { "five_hour": { "utilization": 27.0, "resets_at": "2026-09-16T19:50:00+00:00" },
///   "seven_day": { "utilization": 48.0, "resets_at": "2026-09-20T21:59:59+00:00" } }
/// ```
///
/// Every field is read defensively: anything missing or renamed yields `None` for
/// that window rather than an error. Returns `None` when neither window is present,
/// so a shape change falls back to the local estimate instead of showing 0%.
pub fn parse(json: &str, on_warn: &mut impl FnMut(String)) -> Option<OfficialUsage> {
    let root: Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(err) => {
            on_warn(format!("Could not parse usage response: {err}"));
            return None;
        }
    };

    if !root.is_object() {
        return None;
    }

    let session = read_window(&root, "five_hour");
    let weekly = read_window(&root, "seven_day");

    if session.is_none() && weekly.is_none() {
        on_warn("Usage endpoint returned no recognised windows.".into());
        return None;
    }

    Some(OfficialUsage { session, weekly })
}

/// The endpoint reports utilization as a percentage (27.0 == 27%).
fn read_window(root: &Value, name: &str) -> Option<UsageWindow> {
    let node = root.get(name)?;
    if !node.is_object() {
        return None;
    }

    let percent = node.get("utilization")?.as_f64()?;

    let reset_at = node
        .get("resets_at")
        .and_then(Value::as_str)
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc));

    Some(UsageWindow::new(percent / 100.0, reset_at))
}
