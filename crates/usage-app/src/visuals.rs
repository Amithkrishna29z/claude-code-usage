//! Shared formatting + colour rules used by both the tray icon and the widget, so
//! they never disagree. The ring is a traffic light on how much of the window is
//! gone: green < 70%, orange 70–90%, red > 90%.

use chrono::{DateTime, Duration, Local, Utc};
use usage_core::{UsageSnapshot, UsageSource, UsageState, UsageWindow};

/// Plenty left (under 70% of the window used).
pub const GREEN: [u8; 3] = [0x33, 0xC0, 0x59];
/// Getting close (70–90%).
pub const ORANGE: [u8; 3] = [0xEE, 0x80, 0x33];
/// Nearly exhausted (over 90%).
pub const RED: [u8; 3] = [0xE0, 0x40, 0x40];
/// Unknown / inactive.
pub const GREY: [u8; 3] = [0x88, 0x88, 0x88];

/// Threshold colour for a single window. Grey when the window is unknown.
pub fn color_for(window: Option<UsageWindow>) -> [u8; 3] {
    match window {
        None => GREY,
        Some(w) if w.utilization < 0.70 => GREEN,
        Some(w) if w.utilization < 0.90 => ORANGE,
        Some(_) => RED,
    }
}

/// Colour of the snapshot's session window — the app's primary signal.
pub fn session_color(snapshot: &UsageSnapshot) -> [u8; 3] {
    if snapshot.state == UsageState::Ok {
        color_for(snapshot.session)
    } else {
        GREY
    }
}

/// Compact token count, e.g. 8.3k, 2.1M.
pub fn format_tokens(tokens: i64) -> String {
    if tokens >= 1_000_000 {
        format!("{}M", trim_zeros(tokens as f64 / 1_000_000.0, 2))
    } else if tokens >= 1_000 {
        format!("{}k", trim_zeros(tokens as f64 / 1_000.0, 1))
    } else {
        tokens.to_string()
    }
}

/// Formats to at most `places` decimals, dropping trailing zeros: 16.08, 2.1, 20.
fn trim_zeros(value: f64, places: usize) -> String {
    let text = format!("{value:.places$}");
    if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.').to_owned()
    } else {
        text
    }
}

/// "42%" for a known window, "—" otherwise.
pub fn format_percent(window: Option<UsageWindow>) -> String {
    match window {
        Some(w) => format!("{}%", (w.utilization * 100.0).round() as i64),
        None => "—".to_owned(),
    }
}

/// "1h 47m", "12m 34s", or "45s".
///
/// Seconds appear under an hour so the countdown visibly ticks: the official
/// percentages can only refresh every few minutes, and a display that never moves
/// reads as frozen even when the app is working perfectly.
pub fn format_duration(span: Option<Duration>) -> String {
    let Some(d) = span else {
        return "—".to_owned();
    };

    let total = d.num_seconds().max(0);
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    let seconds = total % 60;

    if hours >= 1 {
        format!("{hours}h {minutes}m")
    } else if minutes >= 1 {
        format!("{minutes}m {seconds}s")
    } else {
        format!("{seconds}s")
    }
}

/// Reset text scaled to the distance: minutes/hours for the session window, a weekday
/// and local time once it is more than a day out (as the weekly window is).
pub fn format_reset(reset_at: Option<DateTime<Utc>>, now: DateTime<Utc>) -> String {
    let Some(at) = reset_at else {
        return "—".to_owned();
    };

    let remaining = at - now;
    if remaining < Duration::days(1) {
        let clamped = if remaining < Duration::zero() {
            Duration::zero()
        } else {
            remaining
        };
        return format!("in {}", format_duration(Some(clamped)));
    }

    at.with_timezone(&Local)
        .format("%a %-l:%M%P")
        .to_string()
        .to_lowercase()
}

/// Relative "updated 12s ago" style text.
pub fn format_relative(when: Option<DateTime<Utc>>, now: DateTime<Utc>) -> String {
    let Some(when) = when else {
        return "never".to_owned();
    };
    let d = now - when;
    let d = if d < Duration::zero() {
        Duration::zero()
    } else {
        d
    };

    if d.num_seconds() < 60 {
        format!("{}s ago", d.num_seconds())
    } else if d.num_minutes() < 60 {
        format!("{}m ago", d.num_minutes())
    } else if d.num_hours() < 24 {
        format!("{}h ago", d.num_hours())
    } else {
        format!("{}d ago", d.num_days())
    }
}

/// Human status line for the given snapshot state.
pub fn state_caption(snapshot: &UsageSnapshot) -> &'static str {
    match snapshot.state {
        UsageState::Ok => {
            if snapshot.source == UsageSource::Official {
                ""
            } else {
                "est."
            }
        }
        UsageState::NoActiveSession => "no session",
        UsageState::NoData => "no data",
        UsageState::NoLogsFound => "no logs",
    }
}

/// The full picture, for the tray tooltip. The widget face is deliberately minimal,
/// so this is where the detail lives — and it must name the data source, because an
/// estimate should never be mistaken for the real figure just because the card is
/// quiet.
pub fn detail_text(s: &UsageSnapshot, now: DateTime<Utc>) -> String {
    if s.state != UsageState::Ok {
        return format!("Claude Code Usage — {}", state_caption_verbose(s));
    }

    let mut lines = vec![format!(
        "Session  {}   resets {}",
        format_percent(s.session),
        format_reset(s.reset_at(), now)
    )];

    if let Some(weekly) = s.weekly {
        lines.push(format!(
            "Weekly   {}   resets {}",
            format_percent(Some(weekly)),
            format_reset(weekly.reset_at, now)
        ));
    }

    lines.push(match s.source {
        UsageSource::Official => "Official figures from Anthropic".to_owned(),
        UsageSource::LocalLogs => format!(
            "Local estimate — {} of {} tokens",
            format_tokens(s.tokens_used),
            format_tokens(s.token_limit)
        ),
    });

    if s.last_activity.is_some() {
        lines.push(format!(
            "Last activity {}",
            format_relative(s.last_activity, now)
        ));
    }

    lines.join(
        "
",
    )
}

/// Spelled-out state for the tray, where there is room for real words.
fn state_caption_verbose(s: &UsageSnapshot) -> &'static str {
    match s.state {
        UsageState::Ok => "active",
        UsageState::NoActiveSession => "no active session",
        UsageState::NoData => "no data yet",
        UsageState::NoLogsFound => "no .claude logs found",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_shows_seconds_under_an_hour_so_it_visibly_ticks() {
        assert_eq!(format_duration(Some(Duration::seconds(45))), "45s");
        assert_eq!(format_duration(Some(Duration::seconds(94))), "1m 34s");
        assert_eq!(format_duration(Some(Duration::minutes(12))), "12m 0s");
    }

    #[test]
    fn duration_drops_seconds_once_hours_are_involved() {
        assert_eq!(format_duration(Some(Duration::minutes(107))), "1h 47m");
        assert_eq!(format_duration(Some(Duration::hours(2))), "2h 0m");
    }

    #[test]
    fn duration_never_renders_negative_time() {
        assert_eq!(format_duration(Some(Duration::seconds(-30))), "0s");
        assert_eq!(format_duration(None), "—");
    }

    #[test]
    fn tokens_are_compact() {
        assert_eq!(format_tokens(950), "950");
        assert_eq!(format_tokens(8_300), "8.3k");
        assert_eq!(format_tokens(20_000_000), "20M");
        assert_eq!(format_tokens(16_081_234), "16.08M");
    }

    #[test]
    fn thresholds_follow_the_traffic_light() {
        let at = |u| Some(UsageWindow::new(u, None));
        assert_eq!(color_for(at(0.69)), GREEN);
        assert_eq!(color_for(at(0.70)), ORANGE);
        assert_eq!(color_for(at(0.89)), ORANGE);
        assert_eq!(color_for(at(0.90)), RED);
        assert_eq!(color_for(at(1.50)), RED);
        assert_eq!(color_for(None), GREY);
    }
}
