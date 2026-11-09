# Daily question

## Current behavior

The app can ask the locally installed `claude` CLI for one reflective question per
local calendar day, then show it in a native desktop notification. The default is
06:00 local time. This feature is independent of the usage monitor: it does not need
OAuth credentials or session logs, but it does require a runnable, authenticated
Claude CLI.

The scheduler is ticked from `WidgetApp::update`, including when the widget is parked
off-screen. It uses the local wall clock, so a sleeping computer or an app started
late in the day will generate the still-unasked question as soon as the app runs after
the scheduled time. It deliberately does not wait until tomorrow when today's question
was missed.

## Configuration and menu

`AppConfig` persists two fields:

- `daily_question_time`: local 24-hour `HH:MM`; default `"06:00"`. Empty/whitespace
  disables scheduling. A non-empty malformed value falls back to 06:00.
- `daily_question_last_asked`: app-written local `YYYY-MM-DD`; empty means never.

The tray's **Daily question** submenu offers **Off**, 5 AM, 6 AM (default), 7 AM,
8 AM, 9 AM, noon, 6 PM, and 9 PM. Any valid minute can still be hand-edited in config.
For a valid non-preset hand-edited time, no tray item is checked by design.

Changing the configured time does not clear the date marker. A question already shown
today is not re-fired by moving the time earlier or later.

## Due rule

For local time `now`, a question is due exactly when all are true:

```text
daily_question_time is not empty
now.time >= parsed daily_question_time
daily_question_last_asked != today as YYYY-MM-DD
no generation is already pending
no CLI failure retry delay remains
```

When Claude returns a non-empty successful result, the UI posts it and writes today's
date marker to config. The marker prevents duplicate generation after restart. The
feature has no question history and does not record answers.

## Claude CLI and notification path

`question.rs` starts a named `daily-question` thread and invokes Claude without a
shell:

```text
claude -p "Give me one random thought-provoking question ..."
```

The prompt requires a single sentence with no preamble or quotation marks. Process
stdout is trimmed and must be non-empty; a non-zero status, spawn failure, or empty
output is a failed attempt. On Windows the child receives `CREATE_NO_WINDOW` so an
invisible release app does not flash a console.

The thread sends either question text or an error back through `mpsc`. The UI thread
shows a `notify-rust` notification with app name **Claude Code Usage**, summary
**Daily question**, and the Claude text as its body. This placement is intentional:
macOS notification delivery must occur on the UI thread.

There is currently no child-process timeout. A hung Claude process leaves a generation
pending for that app run. Do not claim otherwise in user documentation; adding a
timeout requires a deliberate process-lifecycle design and tests.

## Failure behavior

- Failed CLI execution logs a concise stderr message and does not advance the saved
  date marker.
- It waits ten minutes before another attempt, instead of dropping the whole day.
- Notification failure is logged but does not retry or undo the successful date marker:
  Claude already produced the question.
- The notification body is passed as a notification value, never interpolated into a
  shell command.

## Tests and safe changes

Existing module tests cover before-time behavior, after-time/late-start behavior,
once-per-date behavior, Off mode, malformed time fallback, preset parseability, and
default/preset alignment. Keep them deterministic by testing `due` with fixed naive
local datetimes rather than the system clock.

If changing this feature, update `question.rs`, `AppConfig`, tray preset/tick handling,
the configuration document, this page, and direct tests together. Be especially
careful with local-date persistence, `pending` versus retry state, and platform-native
notification/thread constraints.
