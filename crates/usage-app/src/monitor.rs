//! Produces [`UsageSnapshot`]s from Anthropic's official utilization endpoint,
//! falling back to the local-log estimate whenever that endpoint is unavailable.
//! Watches the Claude logs directory to know when to refresh (and for the freshness
//! line, which the endpoint does not provide).
//!
//! Everything runs on a worker thread; the UI only ever receives finished snapshots
//! over a channel, so a slow disk or a hung request can never stall a frame.

use std::path::Path;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::Duration as StdDuration;

use chrono::{DateTime, Duration, Utc};
use notify::{RecursiveMode, Watcher};
use usage_core::models::{OfficialUsage, UsageSnapshot, UsageSource, UsageState};
use usage_core::{calculator, oauth::OAuthUsageClient, reader, AppConfig};

/// The usage endpoint rate-limits hard, and log writes can fire a refresh every
/// couple of seconds, so the official figures are cached: fetched at most this often,
/// and kept serving for [`CACHE_TTL`] after a failure so one blip does not flip the
/// widget over to the local estimate. Five minutes is ample resolution for a 5-hour
/// and a 7-day window.
const MIN_FETCH_INTERVAL: i64 = 300;
const CACHE_TTL: i64 = 900;

/// Safety net when no file events arrive.
const POLL_INTERVAL: StdDuration = StdDuration::from_secs(60);
/// Coalesce a burst of log writes into a single refresh.
const DEBOUNCE: StdDuration = StdDuration::from_millis(1500);

/// Sent to the worker to make something happen.
enum Command {
    Refresh,
    Reconfigure(Box<AppConfig>),
    Shutdown,
}

pub struct UsageMonitor {
    commands: Sender<Command>,
    snapshots: Receiver<UsageSnapshot>,
    /// Kept alive so the watcher keeps firing; dropping it stops file events.
    _watcher: Option<notify::RecommendedWatcher>,
}

impl UsageMonitor {
    /// Spawns the worker and starts watching. `on_wake` is called whenever a new
    /// snapshot is ready, so the UI can request a repaint from its own thread.
    pub fn start(config: AppConfig, on_wake: impl Fn() + Send + 'static) -> Self {
        let (command_tx, command_rx) = mpsc::channel();
        let (snapshot_tx, snapshot_rx) = mpsc::channel();

        let claude_dir = reader::resolve_claude_dir(&config.claude_dir);
        let watcher = spawn_watcher(&claude_dir, command_tx.clone());

        std::thread::Builder::new()
            .name("usage-monitor".into())
            .spawn(move || worker(config, command_rx, snapshot_tx, on_wake))
            .expect("spawn monitor thread");

        // Prime the display immediately rather than waiting for the first tick.
        let _ = command_tx.send(Command::Refresh);

        Self {
            commands: command_tx,
            snapshots: snapshot_rx,
            _watcher: watcher,
        }
    }

    /// Non-blocking: returns the most recent snapshot if one has arrived, discarding
    /// any older ones that queued up behind it.
    pub fn latest(&self) -> Option<UsageSnapshot> {
        let mut newest = None;
        while let Ok(snapshot) = self.snapshots.try_recv() {
            newest = Some(snapshot);
        }
        newest
    }

    pub fn refresh_now(&self) {
        let _ = self.commands.send(Command::Refresh);
    }

    pub fn reconfigure(&self, config: AppConfig) {
        let _ = self.commands.send(Command::Reconfigure(Box::new(config)));
    }
}

impl Drop for UsageMonitor {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Shutdown);
    }
}

/// Watches `{claude_dir}/projects` for log writes. A missing directory is not an
/// error — the poll timer keeps checking, so the app works before Claude Code has
/// ever run.
fn spawn_watcher(
    claude_dir: &Path,
    commands: Sender<Command>,
) -> Option<notify::RecommendedWatcher> {
    let projects = reader::projects_dir(claude_dir);
    if !projects.exists() {
        return None;
    }

    let mut last_sent = std::time::Instant::now() - DEBOUNCE;
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        let Ok(event) = event else { return };
        if !event.paths.iter().any(|p| is_jsonl(p)) {
            return;
        }
        // Debounce: one refresh per burst of writes.
        if last_sent.elapsed() < DEBOUNCE {
            return;
        }
        last_sent = std::time::Instant::now();
        let _ = commands.send(Command::Refresh);
    })
    .ok()?;

    watcher.watch(&projects, RecursiveMode::Recursive).ok()?;
    Some(watcher)
}

fn is_jsonl(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext == "jsonl")
}

fn worker(
    mut config: AppConfig,
    commands: Receiver<Command>,
    snapshots: Sender<UsageSnapshot>,
    on_wake: impl Fn(),
) {
    let mut client = OAuthUsageClient::default();
    let mut cache: Option<(OfficialUsage, DateTime<Utc>)> = None;

    loop {
        match commands.recv_timeout(POLL_INTERVAL) {
            Ok(Command::Shutdown) | Err(RecvTimeoutError::Disconnected) => return,
            Ok(Command::Reconfigure(new_config)) => {
                config = *new_config;
                cache = None; // the directory or the toggle may have changed
            }
            Ok(Command::Refresh) | Err(RecvTimeoutError::Timeout) => {}
        }

        let snapshot = compute(&config, &mut client, &mut cache);
        if snapshots.send(snapshot).is_err() {
            return; // UI is gone
        }
        on_wake();
    }
}

/// Reads the logs (for freshness and as the fallback), then overlays Anthropic's
/// official percentages when the endpoint answers.
fn compute(
    config: &AppConfig,
    client: &mut OAuthUsageClient,
    cache: &mut Option<(OfficialUsage, DateTime<Utc>)>,
) -> UsageSnapshot {
    let now = Utc::now();
    let claude_dir = reader::resolve_claude_dir(&config.claude_dir);

    let read = reader::read_events(&claude_dir, |warning| eprintln!("usage: {warning}"));
    let local = calculator::compute(
        &read.events,
        config.token_limit,
        config.window_hours,
        now,
        read.logs_found,
    );

    if !config.use_official_usage {
        return local;
    }

    let Some(official) = official_usage(config, client, cache, now) else {
        return local;
    };
    let Some(session) = official.session else {
        return local;
    };

    UsageSnapshot {
        state: UsageState::Ok,
        source: UsageSource::Official,
        session: Some(session),
        weekly: official.weekly,
        tokens_used: local.tokens_used,
        token_limit: config.token_limit,
        window_start: None,
        last_activity: local.last_activity,
        generated_at: now,
        events_counted: local.events_counted,
    }
}

/// Returns the official figures, honouring the fetch interval and serving the last
/// good value while it is still fresh enough.
fn official_usage(
    config: &AppConfig,
    client: &mut OAuthUsageClient,
    cache: &mut Option<(OfficialUsage, DateTime<Utc>)>,
    now: DateTime<Utc>,
) -> Option<OfficialUsage> {
    let age = cache.as_ref().map(|(_, at)| now - *at);

    if let (Some(age), Some((cached, _))) = (age, cache.as_ref()) {
        if age < Duration::seconds(MIN_FETCH_INTERVAL) {
            return Some(cached.clone());
        }
    }

    let claude_dir = reader::resolve_claude_dir(&config.claude_dir);
    if let Some(fetched) = client.fetch(&claude_dir, now, |warning| eprintln!("usage: {warning}")) {
        *cache = Some((fetched.clone(), now));
        return Some(fetched);
    }

    // Fetch failed. Keep showing the last good figures until they go properly stale.
    match (age, cache.as_ref()) {
        (Some(age), Some((cached, _))) if age < Duration::seconds(CACHE_TTL) => {
            Some(cached.clone())
        }
        _ => {
            *cache = None;
            None
        }
    }
}
