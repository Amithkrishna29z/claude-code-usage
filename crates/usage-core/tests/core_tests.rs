//! Ported from the original xUnit suite, plus the endpoint-parsing cases. Runs fully
//! offline: no `.claude` logs and no network required.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Duration, TimeZone, Utc};
use usage_core::models::{UsageEvent, UsageState};
use usage_core::{calculator, config::ConfigService, oauth, parser, reader, WidgetStyle};

fn utc(text: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(text)
        .expect("valid timestamp")
        .with_timezone(&Utc)
}

/// A cutoff far enough back that every log counts as in range — for the cases that
/// are about what the reader parses, not which files it reaches for.
fn epoch() -> DateTime<Utc> {
    utc("1970-01-01T00:00:00Z")
}

fn evt(timestamp: &str, total: i64) -> UsageEvent {
    UsageEvent {
        timestamp: utc(timestamp),
        message_id: None,
        // Put the whole amount in one component; total_tokens sums them.
        input_tokens: total,
        output_tokens: 0,
        cache_creation_tokens: 0,
        cache_read_tokens: 0,
    }
}

fn fixture_lines() -> Vec<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("sample-usage.jsonl");
    std::fs::read_to_string(path)
        .expect("fixture readable")
        .lines()
        .map(str::to_owned)
        .collect()
}

/// A temp directory that cleans itself up, so tests leave no trace.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let unique = format!(
            "ccu-{tag}-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let path = std::env::temp_dir().join(unique);
        std::fs::create_dir_all(&path).expect("temp dir");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn usage_line(id: &str, ts: &str, input: i64) -> String {
    format!(
        r#"{{"type":"assistant","timestamp":"{ts}","message":{{"id":"{id}","usage":{{"input_tokens":{input},"output_tokens":0,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}}}}"#
    )
}

// ---- parser ----

#[test]
fn parse_lines_ignores_non_usage_and_malformed_lines() {
    let events: Vec<_> = parser::parse_lines(fixture_lines()).collect();
    // 5 usage-bearing lines (m1, m2, m3, m3-dup, m4); the user line and the garbage
    // line are skipped. De-duplication happens in the reader, not here.
    assert_eq!(events.len(), 5);
}

#[test]
fn parse_lines_sums_all_four_token_components() {
    let events: Vec<_> = parser::parse_lines(fixture_lines()).collect();
    let m2 = events
        .iter()
        .find(|e| e.timestamp == utc("2026-08-15T10:00:00Z"))
        .expect("m2 present");

    assert_eq!(m2.input_tokens, 1000);
    assert_eq!(m2.output_tokens, 500);
    assert_eq!(m2.cache_creation_tokens, 2000);
    assert_eq!(m2.cache_read_tokens, 4000);
    assert_eq!(m2.total_tokens(), 7500);
}

#[test]
fn parse_line_treats_missing_token_fields_as_zero() {
    // m4 in the fixture carries only input_tokens and output_tokens.
    let events: Vec<_> = parser::parse_lines(fixture_lines()).collect();
    let m4 = events
        .iter()
        .find(|e| e.message_id.as_deref() == Some("m4"))
        .expect("m4 present");

    assert_eq!(m4.cache_creation_tokens, 0);
    assert_eq!(m4.cache_read_tokens, 0);
    assert_eq!(m4.total_tokens(), 15);
}

#[test]
fn parse_line_rejects_lines_without_a_usage_block() {
    assert!(parser::parse_line("").is_none());
    assert!(parser::parse_line("not json").is_none());
    assert!(parser::parse_line(r#"{"type":"user","timestamp":"2026-08-15T10:00:00Z"}"#).is_none());
    // A usage block but no timestamp is unusable.
    assert!(parser::parse_line(r#"{"message":{"usage":{"input_tokens":5}}}"#).is_none());
}

// ---- calculator ----

#[test]
fn active_window_starts_after_a_gap_greater_than_five_hours() {
    let events = vec![
        evt("2026-08-15T00:00:00Z", 1350), // block A — excluded (10h gap follows)
        evt("2026-08-15T10:00:00Z", 7500), // block B — active
        evt("2026-08-15T10:30:00Z", 800),  // block B
    ];
    let now = utc("2026-08-15T11:00:00Z");

    let snap = calculator::compute(&events, 20_000, 5.0, now, true);

    assert_eq!(snap.state, UsageState::Ok);
    assert_eq!(snap.tokens_used, 8300);
    assert_eq!(snap.events_counted, 2);
    assert_eq!(snap.window_start, Some(utc("2026-08-15T10:00:00Z")));
    assert_eq!(snap.reset_at(), Some(utc("2026-08-15T15:00:00Z")));
    assert_eq!(snap.time_until_reset(now), Some(Duration::hours(4)));
}

#[test]
fn percent_used_is_tokens_over_limit() {
    let events = vec![evt("2026-08-15T10:00:00Z", 5000)];
    let now = utc("2026-08-15T11:00:00Z");

    let snap = calculator::compute(&events, 20_000, 5.0, now, true);

    assert!((snap.percent_used() - 0.25).abs() < 1e-6);
}

#[test]
fn window_that_has_fully_elapsed_reports_no_active_session() {
    let events = vec![evt("2026-08-15T10:00:00Z", 5000)];
    let now = utc("2026-08-15T16:00:00Z"); // past the 15:00 reset

    let snap = calculator::compute(&events, 20_000, 5.0, now, true);

    assert_eq!(snap.state, UsageState::NoActiveSession);
    assert_eq!(snap.tokens_used, 0);
    assert_eq!(snap.time_until_reset(now), None);
    assert_eq!(snap.last_activity, Some(events[0].timestamp));
}

#[test]
fn no_events_reports_no_data() {
    let snap = calculator::compute(&[], 20_000, 5.0, Utc::now(), true);
    assert_eq!(snap.state, UsageState::NoData);
}

#[test]
fn logs_not_found_is_surfaced() {
    let snap = calculator::compute(&[], 20_000, 5.0, Utc::now(), false);
    assert_eq!(snap.state, UsageState::NoLogsFound);
}

#[test]
fn time_until_reset_never_negative() {
    let events = vec![evt("2026-08-15T10:00:00Z", 5000)];
    let now = utc("2026-08-15T14:59:00Z");

    let snap = calculator::compute(&events, 20_000, 5.0, now, true);
    let remaining = snap.time_until_reset(now).expect("still active");

    assert!(remaining >= Duration::zero());
}

#[test]
fn zero_token_limit_does_not_divide_by_zero() {
    let events = vec![evt("2026-08-15T10:00:00Z", 5000)];
    let snap = calculator::compute(&events, 0, 5.0, utc("2026-08-15T11:00:00Z"), true);
    assert_eq!(snap.percent_used(), 0.0);
}

// ---- reader ----

#[test]
fn read_events_reports_no_logs_when_projects_dir_missing() {
    let temp = TempDir::new("noproj");
    let result = reader::read_events(&temp.path().join("does-not-exist"), epoch(), |_| {});

    assert!(!result.logs_found);
    assert!(result.events.is_empty());
}

#[test]
fn read_events_dedupes_same_message_id_across_files() {
    let temp = TempDir::new("dedupe");
    let proj_a = temp.path().join("projects").join("proj-a");
    let proj_b = temp.path().join("projects").join("proj-b");
    std::fs::create_dir_all(&proj_a).unwrap();
    std::fs::create_dir_all(&proj_b).unwrap();

    std::fs::write(
        proj_a.join("s1.jsonl"),
        format!(
            "{}\n{}\n",
            usage_line("dup", "2026-08-15T10:00:00Z", 100),
            usage_line("unique-a", "2026-08-15T10:01:00Z", 200)
        ),
    )
    .unwrap();

    // The same "dup" id appears again in another file (resumed session).
    std::fs::write(
        proj_b.join("s2.jsonl"),
        format!(
            "{}\n{}\n",
            usage_line("dup", "2026-08-15T10:00:00Z", 100),
            usage_line("unique-b", "2026-08-15T10:02:00Z", 300)
        ),
    )
    .unwrap();

    let result = reader::read_events(temp.path(), epoch(), |_| {});

    assert!(result.logs_found);
    assert_eq!(result.events.len(), 3); // dup counted once
    let total: i64 = result.events.iter().map(|e| e.total_tokens()).sum();
    assert_eq!(total, 600);
}

#[test]
fn read_events_skips_logs_untouched_since_the_cutoff() {
    let temp = TempDir::new("cutoff");
    let proj = temp.path().join("projects").join("proj");
    std::fs::create_dir_all(&proj).unwrap();
    std::fs::write(
        proj.join("s1.jsonl"),
        format!("{}
", usage_line("m1", "2026-08-15T10:00:00Z", 100)),
    )
    .unwrap();

    // The file was written just now, so a cutoff an hour ahead puts it out of range
    // the same way an old log falls behind a cutoff one window back.
    let result = reader::read_events(temp.path(), Utc::now() + Duration::hours(1), |_| {});

    // Not parsed -- but the tree plainly has logs in it, which is a different thing
    // from having none, and the caption depends on telling them apart.
    assert!(result.logs_found);
    assert!(result.events.is_empty());
}

#[test]
fn resolve_claude_dir_falls_back_to_home() {
    let explicit = reader::resolve_claude_dir("  /tmp/somewhere  ");
    assert_eq!(explicit, PathBuf::from("/tmp/somewhere"));

    let default = reader::resolve_claude_dir("");
    assert!(default.ends_with(".claude"));
}

// ---- oauth ----

/// Trimmed from a real `/api/oauth/usage` response, keeping the fields we read plus
/// enough neighbours (nulls, unknown keys) to prove they are ignored safely.
const REAL_SHAPE: &str = r#"
{
  "five_hour": {
    "utilization": 27.0,
    "resets_at": "2026-09-16T19:50:00.198829+00:00",
    "limit_dollars": null
  },
  "seven_day": {
    "utilization": 48.0,
    "resets_at": "2026-09-20T21:59:59.198853+00:00",
    "locked_reason": null
  },
  "seven_day_opus": null,
  "nimbus_quill": { "utilization": 0.0, "resets_at": null },
  "extra_usage": { "is_enabled": false, "used_credits": 0.0 }
}
"#;

#[test]
fn parse_reads_both_windows_as_fractions() {
    let usage = oauth::parse(REAL_SHAPE, &mut |_| {}).expect("parsed");

    assert!((usage.session.unwrap().utilization - 0.27).abs() < 1e-9);
    assert!((usage.weekly.unwrap().utilization - 0.48).abs() < 1e-9);
}

#[test]
fn parse_normalizes_reset_times_to_utc() {
    let usage = oauth::parse(REAL_SHAPE, &mut |_| {}).expect("parsed");

    let session_reset = usage.session.unwrap().reset_at.expect("reset present");
    assert_eq!(
        session_reset.date_naive(),
        Utc.with_ymd_and_hms(2026, 9, 16, 19, 50, 0)
            .unwrap()
            .date_naive()
    );
    assert_eq!(session_reset.time().to_string(), "19:50:00.198829");
}

#[test]
fn parse_returns_session_only_when_weekly_is_absent() {
    let usage =
        oauth::parse(r#"{ "five_hour": { "utilization": 5.0 } }"#, &mut |_| {}).expect("parsed");

    assert!((usage.session.unwrap().utilization - 0.05).abs() < 1e-9);
    assert!(usage.weekly.is_none());
    assert!(usage.session.unwrap().reset_at.is_none());
}

#[test]
fn parse_returns_none_rather_than_a_misleading_zero() {
    let cases = [
        "not json at all",
        "[]",
        r#"{ "five_hour": null, "seven_day": null }"#, // both windows nulled out
        r#"{ "renamed_window": { "utilization": 42.0 } }"#, // endpoint shape changed
        r#"{ "five_hour": { "resets_at": "2026-09-16T19:50:00Z" } }"#, // no utilization
    ];

    for case in cases {
        assert!(
            oauth::parse(case, &mut |_| {}).is_none(),
            "should not parse: {case}"
        );
    }
}

#[test]
fn read_access_token_returns_none_when_credentials_are_missing() {
    let temp = TempDir::new("creds");

    assert!(oauth::read_access_token(temp.path()).is_none());

    // Present but not the shape we expect — still none, never a panic.
    std::fs::write(
        oauth::credentials_path(temp.path()),
        r#"{ "somethingElse": 1 }"#,
    )
    .unwrap();
    assert!(oauth::read_access_token(temp.path()).is_none());

    std::fs::write(oauth::credentials_path(temp.path()), "{ corrupt").unwrap();
    assert!(oauth::read_access_token(temp.path()).is_none());
}

#[test]
fn read_access_token_reads_the_claude_code_credential_shape() {
    let temp = TempDir::new("credsok");
    std::fs::write(
        oauth::credentials_path(temp.path()),
        r#"{ "claudeAiOauth": { "accessToken": "tok-123", "expiresAt": 1789570200 } }"#,
    )
    .unwrap();

    assert_eq!(
        oauth::read_access_token(temp.path()).as_deref(),
        Some("tok-123")
    );
}

#[test]
fn client_respects_its_backoff_without_sending_a_request() {
    let temp = TempDir::new("backoff");
    // Point at an unroutable endpoint: if the backoff were ignored, this would try to
    // connect. The credentials are missing too, so either way it must not panic.
    let mut client = oauth::OAuthUsageClient::new("http://127.0.0.1:9/never");
    let result = client.fetch(temp.path(), Utc::now(), |_| {});
    assert!(result.is_none());
}

// ---- config ----

#[test]
fn load_returns_defaults_when_file_is_missing_or_corrupt() {
    let temp = TempDir::new("config");
    let service = ConfigService::with_dir(temp.path());

    let fresh = service.load();
    assert!(fresh.use_official_usage);
    assert_eq!(fresh.token_limit, 20_000_000);
    assert_eq!(fresh.window_hours, 5.0);

    std::fs::write(service.config_path(), "{ not valid json").unwrap();
    assert_eq!(service.load().token_limit, 20_000_000);
}

#[test]
fn save_then_load_round_trips() {
    let temp = TempDir::new("roundtrip");
    let service = ConfigService::with_dir(temp.path());

    let mut config = service.load();
    config.token_limit = 1234;
    config.use_official_usage = false;
    config.widget_left = Some(42.5);
    config.claude_dir = "/custom/.claude".into();
    service.save(&config).expect("saved");

    let loaded = service.load();
    assert_eq!(loaded.token_limit, 1234);
    assert!(!loaded.use_official_usage);
    assert_eq!(loaded.widget_left, Some(42.5));
    assert_eq!(loaded.claude_dir, "/custom/.claude");
}

#[test]
fn unknown_and_missing_keys_do_not_break_loading() {
    let temp = TempDir::new("partial");
    let service = ConfigService::with_dir(temp.path());
    std::fs::create_dir_all(temp.path()).unwrap();

    // A config written by a future version, and one written by an older one.
    std::fs::write(
        service.config_path(),
        r#"{ "token_limit": 99, "something_new": true }"#,
    )
    .unwrap();

    let loaded = service.load();
    assert_eq!(loaded.token_limit, 99);
    assert!(loaded.use_official_usage); // defaulted, not lost
}

#[test]
fn config_with_a_utf8_bom_still_loads() {
    // Notepad and PowerShell's `Set-Content -Encoding utf8` both prepend a BOM on
    // Windows. serde_json rejects it, which used to silently discard every setting
    // in a hand-edited file — and the app then overwrote the file with defaults.
    let temp = TempDir::new("bom");
    let service = ConfigService::with_dir(temp.path());

    let body = br#"{ "token_limit": 777, "use_official_usage": false }"#;
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend_from_slice(body);
    std::fs::write(service.config_path(), bytes).unwrap();

    let loaded = service.load();
    assert_eq!(loaded.token_limit, 777);
    assert!(!loaded.use_official_usage);
    assert!(service.is_readable());
}

#[test]
fn style_round_trips_and_survives_a_typo() {
    let temp = TempDir::new("style");
    let service = ConfigService::with_dir(temp.path());

    assert_eq!(service.load().style, WidgetStyle::Rings);

    let mut config = service.load();
    config.style = WidgetStyle::Pill;
    config.token_limit = 4321;
    service.save(&config).unwrap();
    assert_eq!(service.load().style, WidgetStyle::Pill);

    // A mistyped style must not take the rest of the file down with it: the style
    // falls back to the default and every other setting is still honoured.
    std::fs::write(
        service.config_path(),
        r#"{"style": "sparkles", "token_limit": 777}"#,
    )
    .unwrap();
    assert!(service.is_readable());
    assert_eq!(service.load().style, WidgetStyle::Rings);
    assert_eq!(service.load().token_limit, 777);
}

#[test]
fn unparseable_config_is_reported_as_unreadable() {
    // The app checks this before saving, so a malformed file the user is mid-edit
    // never gets clobbered with defaults.
    let temp = TempDir::new("unreadable");
    let service = ConfigService::with_dir(temp.path());

    std::fs::write(service.config_path(), "{ half an edit").unwrap();
    assert!(!service.is_readable());
    assert_eq!(service.load().token_limit, 20_000_000); // defaults, but non-fatal

    // A file that simply does not exist is readable in this sense: writing is safe.
    let fresh = TempDir::new("fresh");
    assert!(ConfigService::with_dir(fresh.path()).is_readable());
}
