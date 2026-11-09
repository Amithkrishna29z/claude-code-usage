//! The daily question: once per local calendar day, at the configured time, ask the
//! installed Claude CLI for one thought-provoking question and show it as a native
//! desktop notification.
//!
//! The CLI call is a process spawn that takes a few seconds, so it runs on a thread of
//! its own and the finished text comes back over a channel. The notification itself is
//! raised from [`DailyQuestion::tick`], which runs on the UI thread — macOS will not
//! post a notification from anywhere else.

use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use chrono::{Local, NaiveDate, NaiveDateTime, NaiveTime};
use usage_core::{AppConfig, DEFAULT_QUESTION_TIME};

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

pub struct DailyQuestion {
    answers: Receiver<Result<String, String>>,
    /// Cloned into each generation thread.
    sender: Sender<Result<String, String>>,
    /// The day a generation is in flight for, so a second one is not started while
    /// the CLI is still thinking.
    pending: Option<NaiveDate>,
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
    /// Returns the day the shown question was for, which the caller records in the
    /// config so a restart does not ask again for the same day.
    pub fn tick(&mut self, config: &AppConfig) -> Option<NaiveDate> {
        if let Some(day) = self.pending {
            return match self.answers.try_recv() {
                Ok(Ok(question)) => {
                    self.pending = None;
                    notify(&question);
                    Some(day)
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

        let day = due(config, Local::now().naive_local())?;
        self.pending = Some(day);
        self.retry_at = None;
        spawn(self.sender.clone());
        None
    }
}

/// The day a question is owed for, if one is.
///
/// The comparison is against the wall clock rather than a timer, which is what makes
/// this survive a restart, a sleeping laptop and the clocks going forward: whenever
/// the app is running and the day's time has passed unanswered, the question goes out
/// then. So starting the app for the first time in the afternoon asks straight away
/// rather than waiting until tomorrow — the day's question has not been asked.
fn due(config: &AppConfig, now: NaiveDateTime) -> Option<NaiveDate> {
    if config.daily_question_time.trim().is_empty() {
        return None;
    }
    if now.time() < parse_time(&config.daily_question_time) {
        return None;
    }
    let today = now.date();
    if config.daily_question_last_asked == today.to_string() {
        return None;
    }
    Some(today)
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
fn ask_claude() -> Result<String, String> {
    let mut command = std::process::Command::new("claude");
    command.arg("-p").arg(PROMPT);
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

    let question = String::from_utf8_lossy(&output.stdout).trim().to_owned();
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

    #[test]
    fn nothing_is_due_before_the_time() {
        assert_eq!(due(&config("06:00", ""), at("2026-11-09 05:59")), None);
    }

    #[test]
    fn the_day_is_due_once_the_time_has_passed() {
        let day = NaiveDate::from_ymd_opt(2026, 11, 9).unwrap();
        assert_eq!(due(&config("06:00", ""), at("2026-11-09 06:00")), Some(day));
        // Late is still due: this is what makes it survive a restart.
        assert_eq!(due(&config("06:00", ""), at("2026-11-09 23:30")), Some(day));
    }

    #[test]
    fn a_hand_edited_non_preset_time_uses_its_exact_boundary() {
        let day = NaiveDate::from_ymd_opt(2026, 11, 9).unwrap();
        let config = config("21:30", "");

        assert_eq!(due(&config, at("2026-11-09 21:29")), None);
        assert_eq!(due(&config, at("2026-11-09 21:30")), Some(day));
    }

    #[test]
    fn a_day_already_asked_is_not_due_again() {
        let today = config("06:00", "2026-11-09");
        assert_eq!(due(&today, at("2026-11-09 06:00")), None);
        // ...but the next day is.
        assert_eq!(
            due(&today, at("2026-11-10 06:00")),
            NaiveDate::from_ymd_opt(2026, 11, 10)
        );
    }

    #[test]
    fn changing_the_time_does_not_repeat_a_question_already_shown_today() {
        for time in ["05:00", "21:30"] {
            assert_eq!(
                due(&config(time, "2026-11-09"), at("2026-11-09 23:30")),
                None,
                "changing to {time} must preserve the daily marker"
            );
        }
    }

    #[test]
    fn an_empty_time_is_off() {
        assert_eq!(due(&config("", ""), at("2026-11-09 23:30")), None);
        assert_eq!(due(&config("   ", ""), at("2026-11-09 23:30")), None);
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
        question.pending = Some(NaiveDate::from_ymd_opt(2026, 11, 9).unwrap());
        question.sender.send(Err("offline".to_owned())).unwrap();

        assert_eq!(question.tick(&config("06:00", "")), None);
        assert!(question.pending.is_none());
        assert!(question.retry_at.is_some_and(|at| at > Instant::now()));
    }
}
