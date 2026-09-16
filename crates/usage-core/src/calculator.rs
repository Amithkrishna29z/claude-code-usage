//! Turns a set of [`UsageEvent`]s into a [`UsageSnapshot`] using Anthropic's 5-hour
//! session-block model:
//!
//! * Events are grouped into blocks. A block opens at its first event and lasts
//!   `window_hours`. A new block opens when an event is more than `window_hours`
//!   after the current block's start, OR more than `window_hours` after the previous
//!   event (the ">5h gap" rule).
//! * The active block is the most recent one whose window still covers "now". Its
//!   events determine tokens used, and `start + window_hours` is the reset time.
//! * If the most recent block's window has fully elapsed, usage has reset: the
//!   snapshot reports [`UsageState::NoActiveSession`] with zero tokens.

use chrono::{DateTime, Duration, Utc};

use crate::models::{UsageEvent, UsageSnapshot, UsageSource, UsageState, UsageWindow};

pub fn compute(
    events: &[UsageEvent],
    token_limit: i64,
    window_hours: f64,
    now: DateTime<Utc>,
    logs_found: bool,
) -> UsageSnapshot {
    if !logs_found {
        return UsageSnapshot::empty(UsageState::NoLogsFound, token_limit, now);
    }
    if events.is_empty() {
        return UsageSnapshot::empty(UsageState::NoData, token_limit, now);
    }

    let hours = if window_hours <= 0.0 {
        5.0
    } else {
        window_hours
    };
    let window = Duration::milliseconds((hours * 3_600_000.0) as i64);

    let mut ordered: Vec<&UsageEvent> = events.iter().collect();
    ordered.sort_by_key(|e| e.timestamp);
    let last_activity = ordered[ordered.len() - 1].timestamp;

    // Walk events, accumulating the currently-open block. We only need the block that
    // is active "now", so we track the open block's start and running totals.
    let mut block_start = ordered[0].timestamp;
    let mut prev = block_start;
    let mut block_tokens: i64 = 0;
    let mut block_count: usize = 0;

    for event in &ordered {
        let new_block =
            (event.timestamp - block_start) > window || (event.timestamp - prev) > window;
        if new_block {
            block_start = event.timestamp;
            block_tokens = 0;
            block_count = 0;
        }

        block_tokens += event.total_tokens();
        block_count += 1;
        prev = event.timestamp;
    }

    let reset_at = block_start + window;

    // Is the final block still active at "now"? If now is past its reset, the window
    // has elapsed with no new activity — usage has reset to zero.
    if now >= reset_at {
        return UsageSnapshot {
            state: UsageState::NoActiveSession,
            source: UsageSource::LocalLogs,
            session: Some(UsageWindow::new(0.0, None)),
            tokens_used: 0,
            token_limit,
            last_activity: Some(last_activity),
            generated_at: now,
            ..UsageSnapshot::empty(UsageState::NoActiveSession, token_limit, now)
        };
    }

    let utilization = if token_limit <= 0 {
        0.0
    } else {
        block_tokens as f64 / token_limit as f64
    };

    UsageSnapshot {
        state: UsageState::Ok,
        source: UsageSource::LocalLogs,
        session: Some(UsageWindow::new(utilization, Some(reset_at))),
        weekly: None,
        tokens_used: block_tokens,
        token_limit,
        window_start: Some(block_start),
        last_activity: Some(last_activity),
        generated_at: now,
        events_counted: block_count,
    }
}
