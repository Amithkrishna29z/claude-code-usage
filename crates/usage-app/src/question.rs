//! The daily question: at the configured time each local day, ask the installed
//! Claude CLI for one thought-provoking question and show it as a native desktop
//! notification — then keep asking, once every five hours and five minutes, for as
//! long as the session window stays empty.
//!
//! The repeat interval is the 5-hour session window plus five minutes of margin,
//! which is what makes the cadence self-sustaining: asking spends a little of the
//! allowance and so opens a window, and by the time the next repeat comes round that
//! window has closed again. The empty-window condition is what keeps the repeats out
//! of the user's way — while they are actually using Claude Code the session window
//! is not empty, so the questions stand down until it drains.
//!
//! The CLI call is a process spawn that takes a few seconds, so it runs on a thread of
//! its own and the finished text comes back over a channel. The notification itself is
//! raised from [`DailyQuestion::tick`], which runs on the UI thread — macOS will not
//! post a notification from anywhere else.

use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use chrono::{Local, NaiveDateTime, NaiveTime, TimeDelta};
use usage_core::{AppConfig, UsageSnapshot, DEFAULT_QUESTION_TIME};

/// What the CLI is asked for. It is one shot with no session, so the wording has to
/// carry the whole brief: a notification body has room for a sentence and nothing
/// else, and a preamble ("Here's a question for you:") would eat half of it.
const PROMPT: &str = "Give me one random thought-provoking question to reflect on \
     today. Reply with the question alone: a single sentence, no preamble, no \
     quotation marks.";

/// The times the tray's *Daily question* submenu offers: the value stored in
/// `daily_question_time`, and how the menu names it. `""` is off.
pub const PRESETS: [(&str, &str); 9] = [
    ("", "Off"),
    ("05:00", "5:00 AM"),
    ("06:00", "6:00 AM (default)"),
    ("07:00", "7:00 AM"),
    ("08:00", "8:00 AM"),
    ("09:00", "9:00 AM"),
    ("12:00", "12:00 PM"),
    ("18:00", "6:00 PM"),
    ("21:00", "9:00 PM"),
];

/// How long to leave it before trying again after the CLI fails. A laptop asleep or
/// offline at 6am is the normal case, not an exceptional one, so a failure costs a few
/// minutes rather than the whole day.
const RETRY_AFTER: Duration = Duration::from_secs(600);

/// How long after one question the next is due: the 5-hour session window, plus five
/// minutes so the window a question opens has definitely closed before the next is
/// considered.
const REPEAT_AFTER: TimeDelta = match TimeDelta::try_minutes(5 * 60 + 5) {
    Some(delta) => delta,
    None => panic!("5h05m is a representable duration"),
};

/// How [`AppConfig::daily_question_last_asked_at`] is written and read.
const ASKED_AT_FORMAT: &str = "%Y-%m-%d %H:%M:%S";

pub struct DailyQuestion {
    answers: Receiver<Result<String, String>>,
    /// Cloned into each generation thread.
    sender: Sender<Result<String, String>>,
    /// When the in-flight generation became due, so a second one is not started
    /// while the CLI is still thinking.
    pending: Option<NaiveDateTime>,
    /// Set after a failure; nothing is attempted again until it has passed.
    retry_at: Option<Instant>,
}

impl DailyQuestion {
    pub fn new() -> Self {
        let (sender, answers) = mpsc::channel();
        Self {
            answers,
            sender,
            pending: None,
            retry_at: None,
        }
    }

    /// Call once a frame. Starts a generation when one is due, and shows the question
    /// once it arrives.
    ///
    /// Returns the time the shown question became due, which the caller hands to
    /// [`record`] so that neither a restart nor the next frame asks again.
    pub fn tick(&mut self, config: &AppConfig, snapshot: &UsageSnapshot) -> Option<NaiveDateTime> {
        if let Some(due_at) = self.pending {
            return match self.answers.try_recv() {
                Ok(Ok(question)) => {
                    self.pending = None;
                    notify(&question);
                    Some(due_at)
                }
                Ok(Err(err)) => {
                    eprintln!("usage: no daily question ({err}); trying again shortly.");
                    self.pending = None;
                    self.retry_at = Some(Instant::now() + RETRY_AFTER);
                    None
                }
                // Still thinking.
                Err(_) => None,
            };
        }

        if self.retry_at.is_some_and(|at| Instant::now() < at) {
            return None;
        }

        let due_at = due(
            config,
            Local::now().naive_local(),
            session_is_idle(snapshot),
        )?;
        self.pending = Some(due_at);
        self.retry_at = None;
        spawn(self.sender.clone());
        None
    }
}

/// When a question is owed, if one is.
///
/// There are two ways for one to fall due. The day's first question is owed once the
/// configured time has passed with the day unasked, and is owed regardless of what
/// the session window holds: it is the one the user set a time for by name, so an
/// active session does not hold it back. Every question after that is owed five hours
/// and five minutes on from the last one, and only while the session window is empty.
///
/// Both comparisons are against the wall clock rather than a timer, which is what
/// makes this survive a restart, a sleeping laptop and the clocks going forward:
/// whenever the app is running and a question is owed, it goes out then. So starting
/// the app for the first time in the afternoon asks straight away rather than waiting
/// until tomorrow — the day's question has not been asked.
fn due(config: &AppConfig, now: NaiveDateTime, session_is_idle: bool) -> Option<NaiveDateTime> {
    if config.daily_question_time.trim().is_empty() {
        return None;
    }

    let first_of_the_day_is_owed = now.time() >= parse_time(&config.daily_question_time)
        && config.daily_question_last_asked != now.date().to_string();
    if first_of_the_day_is_owed {
        return Some(now);
    }

    // A repeat waits for the session window to drain. Standing down is all it does:
    // the question stays owed, so it goes out on the first frame after the window
    // empties rather than being skipped until the next five-hour mark.
    if !session_is_idle {
        return None;
    }

    // No recorded time means no question has been asked since this field was
    // introduced; the day's first question writes one, and the repeats start from
    // there.
    let last_asked_at = parse_asked_at(&config.daily_question_last_asked_at)?;
    (now - last_asked_at >= REPEAT_AFTER).then_some(now)
}

/// Whether the session window is empty.
///
/// `percent_used` reports zero both for a window with nothing in it and for a
/// snapshot that has no session window at all — no active session, no data yet, no
/// logs — which for this purpose are the same state: none of the allowance is
/// spoken for, so a question is not landing on top of work in progress.
fn session_is_idle(snapshot: &UsageSnapshot) -> bool {
    snapshot.percent_used() <= 0.0
}

/// Records a question as asked: the date holds the day's first question to one a day,
/// and the timestamp starts the clock on the next repeat.
pub fn record(config: &mut AppConfig, asked_at: NaiveDateTime) {
    config.daily_question_last_asked = asked_at.date().to_string();
    config.daily_question_last_asked_at = asked_at.format(ASKED_AT_FORMAT).to_string();
}

/// The recorded time of the last question, or `None` if there is no readable one.
fn parse_asked_at(text: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(text.trim(), ASKED_AT_FORMAT).ok()
}

/// `HH:MM` on a 24-hour clock, falling back to the default for anything unreadable.
fn parse_time(text: &str) -> NaiveTime {
    NaiveTime::parse_from_str(text.trim(), "%H:%M").unwrap_or_else(|_| {
        NaiveTime::parse_from_str(DEFAULT_QUESTION_TIME, "%H:%M")
            .expect("the built-in default is a valid HH:MM")
    })
}

fn spawn(answers: Sender<Result<String, String>>) {
    let spawned = std::thread::Builder::new()
        .name("daily-question".into())
        .spawn(move || {
            let _ = answers.send(ask_claude());
        });
    if let Err(err) = spawned {
        eprintln!("usage: could not start the daily question thread: {err}");
    }
}

/// Runs `claude -p <prompt>` and returns the one line it prints.
///
/// The extra flags strip everything the CLI would otherwise load for a full coding
/// session — the default system prompt, the built-in tools, the skills list — none of
/// which this one-line question needs. Measured against the real CLI, that cuts a cold
/// call from ~28k tokens to ~2.8k: most of what a bare `claude -p` call spends is
/// context the model never uses, not the answer itself.
fn ask_claude() -> Result<String, String> {
    let mut command = std::process::Command::new("claude");
    command
        .arg("-p")
        .arg(PROMPT)
        .arg("--system-prompt")
        .arg("")
        .arg("--tools")
        .arg("")
        .arg("--disable-slash-commands");
    no_console_window(&mut command);

    let output = command
        .output()
        .map_err(|err| format!("could not run the claude CLI: {err}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "claude exited with {}: {}",
            output.status,
            stderr.trim()
        ));
    }

    question_from_stdout(&output.stdout)
}

/// Normalizes Claude's stdout into the one notification-ready question.
///
/// Kept separate from process execution so whitespace and empty-output handling stay
/// deterministic to test without invoking the user's Claude CLI.
fn question_from_stdout(stdout: &[u8]) -> Result<String, String> {
    let question = String::from_utf8_lossy(stdout).trim().to_owned();
    if question.is_empty() {
        return Err("claude printed nothing".to_owned());
    }
    Ok(question)
}

/// `claude.exe` is a console program, and this app has no console of its own in a
/// release build — without this flag every question would flash up a black window.
#[cfg(target_os = "windows")]
fn no_console_window(command: &mut std::process::Command) {
    use std::os::windows::process::CommandExt as _;
    // `CREATE_NO_WINDOW`, from the Win32 process creation flags.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(target_os = "windows"))]
fn no_console_window(_command: &mut std::process::Command) {}

/// Raises the desktop notification. A failure is logged and dropped: there is nothing
/// useful to do about a notification daemon that will not take the message, and the
/// question itself is already gone.
fn notify(question: &str) {
    let shown = notify_rust::Notification::new()
        .appname("Claude Code Usage")
        .summary("Daily question")
        .body(question)
        .show();
    if let Err(err) = shown {
        eprintln!("usage: could not show the daily question: {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use usage_core::{UsageState, UsageWindow};

    fn at(text: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M").expect("test timestamp")
    }

    fn config(time: &str, last_asked: &str) -> AppConfig {
        AppConfig {
            daily_question_time: time.to_owned(),
            daily_question_last_asked: last_asked.to_owned(),
            ..AppConfig::default()
        }
    }

    /// A config whose day is already asked, so only the repeat rule can fire.
    fn repeating(last_asked_at: &str) -> AppConfig {
        let (date, _) = last_asked_at.split_once(' ').expect("date and time");
        AppConfig {
            daily_question_last_asked_at: format!("{last_asked_at}:00"),
            ..config("06:00", date)
        }
    }

    const IDLE: bool = true;
    const IN_USE: bool = false;

    #[test]
    fn nothing_is_due_before_the_time() {
        let before = at("2026-11-09 05:59");
        assert_eq!(due(&config("06:00", ""), before, IDLE), None);
    }

    #[test]
    fn the_day_is_due_once_the_time_has_passed() {
        let on_time = at("2026-11-09 06:00");
        assert_eq!(due(&config("06:00", ""), on_time, IDLE), Some(on_time));
        // Late is still due: this is what makes it survive a restart.
        let late = at("2026-11-09 23:30");
        assert_eq!(due(&config("06:00", ""), late, IDLE), Some(late));
    }

    #[test]
    fn the_days_first_question_is_due_even_while_the_session_is_in_use() {
        let on_time = at("2026-11-09 06:00");
        assert_eq!(due(&config("06:00", ""), on_time, IN_USE), Some(on_time));
    }

    #[test]
    fn a_hand_edited_non_preset_time_uses_its_exact_boundary() {
        let config = config("21:30", "");
        let boundary = at("2026-11-09 21:30");

        assert_eq!(due(&config, at("2026-11-09 21:29"), IDLE), None);
        assert_eq!(due(&config, boundary, IDLE), Some(boundary));
    }

    #[test]
    fn a_day_already_asked_is_not_due_again() {
        let today = config("06:00", "2026-11-09");
        assert_eq!(due(&today, at("2026-11-09 06:00"), IDLE), None);
        // ...but the next day is.
        let tomorrow = at("2026-11-10 06:00");
        assert_eq!(due(&today, tomorrow, IDLE), Some(tomorrow));
    }

    #[test]
    fn changing_the_time_does_not_repeat_a_question_already_shown_today() {
        for time in ["05:00", "21:30"] {
            assert_eq!(
                due(&config(time, "2026-11-09"), at("2026-11-09 23:30"), IDLE),
                None,
                "changing to {time} must preserve the daily marker"
            );
        }
    }

    #[test]
    fn an_empty_time_is_off() {
        let now = at("2026-11-09 23:30");
        assert_eq!(due(&config("", ""), now, IDLE), None);
        assert_eq!(due(&config("   ", ""), now, IDLE), None);
    }

    #[test]
    fn a_repeat_is_due_five_hours_and_five_minutes_after_the_last_question() {
        let config = repeating("2026-11-09 06:00");

        assert_eq!(due(&config, at("2026-11-09 11:04"), IDLE), None);
        let boundary = at("2026-11-09 11:05");
        assert_eq!(due(&config, boundary, IDLE), Some(boundary));
    }

    #[test]
    fn repeats_keep_their_own_cadence_rather_than_the_configured_time() {
        // Each repeat is measured from the one before it, so the questions march on
        // across the day: 06:00, 11:05, 16:10, 21:15.
        for (last, next) in [
            ("2026-11-09 11:05", "2026-11-09 16:10"),
            ("2026-11-09 16:10", "2026-11-09 21:15"),
        ] {
            let next = at(next);
            assert_eq!(due(&repeating(last), next, IDLE), Some(next));
        }
    }

    #[test]
    fn a_repeat_stands_down_while_the_session_window_has_usage_in_it() {
        let config = repeating("2026-11-09 06:00");
        let overdue = at("2026-11-09 11:05");

        assert_eq!(due(&config, overdue, IN_USE), None);
        // It is deferred, not skipped: the moment the window drains it goes out,
        // without waiting for the next five-hour mark.
        assert_eq!(due(&config, overdue, IDLE), Some(overdue));
        let much_later = at("2026-11-09 14:30");
        assert_eq!(due(&config, much_later, IDLE), Some(much_later));
    }

    #[test]
    fn repeats_do_not_start_until_a_question_has_recorded_its_time() {
        // An upgrade from a config written before repeats existed: the day is marked
        // asked but there is no timestamp to count from, so nothing repeats until the
        // next day's first question records one.
        let upgraded = config("06:00", "2026-11-09");
        assert!(upgraded.daily_question_last_asked_at.is_empty());
        assert_eq!(due(&upgraded, at("2026-11-09 23:30"), IDLE), None);
    }

    #[test]
    fn an_unreadable_recorded_time_stops_repeats_rather_than_firing_every_frame() {
        let mut config = repeating("2026-11-09 06:00");
        config.daily_question_last_asked_at = "yesterday".to_owned();
        assert_eq!(due(&config, at("2026-11-09 23:30"), IDLE), None);
    }

    #[test]
    fn turning_the_question_off_stops_the_repeats_too() {
        let mut config = repeating("2026-11-09 06:00");
        config.daily_question_time = String::new();
        assert_eq!(due(&config, at("2026-11-09 11:05"), IDLE), None);
    }

    #[test]
    fn recording_a_question_marks_the_day_and_starts_the_repeat_clock() {
        let mut config = config("06:00", "");
        let asked_at = at("2026-11-09 06:00");

        record(&mut config, asked_at);

        assert_eq!(config.daily_question_last_asked, "2026-11-09");
        assert_eq!(config.daily_question_last_asked_at, "2026-11-09 06:00:00");
        // What was just recorded is no longer due, and the next one is a repeat.
        assert_eq!(due(&config, asked_at, IDLE), None);
        assert_eq!(
            due(&config, at("2026-11-09 11:05"), IDLE),
            Some(at("2026-11-09 11:05"))
        );
    }

    #[test]
    fn only_an_empty_session_window_counts_as_idle() {
        let now = chrono::Utc::now();
        let mut snapshot = UsageSnapshot::empty(UsageState::NoActiveSession, 1_000, now);
        // No window at all: nothing of the allowance is spoken for.
        assert!(session_is_idle(&snapshot));

        snapshot.session = Some(UsageWindow::new(0.0, None));
        assert!(session_is_idle(&snapshot));

        // Any usage at all, however small, holds the repeats back.
        snapshot.session = Some(UsageWindow::new(0.03, None));
        assert!(!session_is_idle(&snapshot));
    }

    #[test]
    fn an_unreadable_time_falls_back_to_the_default() {
        assert_eq!(parse_time("nonsense"), parse_time(DEFAULT_QUESTION_TIME));
        assert_eq!(parse_time("25:00"), parse_time(DEFAULT_QUESTION_TIME));
        assert_eq!(
            Some(parse_time(" 21:30 ")),
            NaiveTime::from_hms_opt(21, 30, 0)
        );
    }

    #[test]
    fn every_preset_is_a_time_the_scheduler_can_read() {
        for (value, _) in PRESETS.iter().filter(|(value, _)| !value.is_empty()) {
            assert_eq!(
                NaiveTime::parse_from_str(value, "%H:%M").ok(),
                Some(parse_time(value)),
                "preset {value} does not parse"
            );
        }
    }

    #[test]
    fn the_default_config_time_is_a_preset() {
        let default = AppConfig::default().daily_question_time;
        assert!(PRESETS.iter().any(|(value, _)| *value == default));
    }

    #[test]
    fn a_cli_failure_defers_another_attempt() {
        let mut question = DailyQuestion::new();
        question.pending = Some(at("2026-11-09 06:00"));
        question.sender.send(Err("offline".to_owned())).unwrap();

        let idle = UsageSnapshot::empty(UsageState::NoActiveSession, 1_000, chrono::Utc::now());
        assert_eq!(question.tick(&config("06:00", ""), &idle), None);
        assert!(question.pending.is_none());
        assert!(question.retry_at.is_some_and(|at| at > Instant::now()));
    }

    #[test]
    fn cli_stdout_is_trimmed_and_empty_output_is_rejected() {
        assert_eq!(
            question_from_stdout(b"  What would make today meaningful?  \n"),
            Ok("What would make today meaningful?".to_owned())
        );
        assert_eq!(
            question_from_stdout(b" \r\n\t "),
            Err("claude printed nothing".to_owned())
        );
    }
}
