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
/// couple of seconds, so the official figures are cached, and kept serving for
/// [`CACHE_TTL`] after a failure so one blip does not flip the widget over to the
/// local estimate.
///
/// How long the cache is served for depends on whether anything has actually
/// happened. An idle widget waits out `refresh_seconds`, because re-asking for
/// numbers that cannot have moved is pure rate-limit budget. New log activity — or a
/// refresh the user asked for — means real usage has been spent and the cached
/// figures are already wrong, so the next fetch goes out as soon as the floor below
/// allows.
/// Floor between fetches. Below this the endpoint starts answering 429, which
/// drops the widget to its local estimate — worse than a slightly stale real number.
const MIN_FETCH_INTERVAL: i64 = 60;
const CACHE_TTL: i64 = 900;

/// How often the worker wakes to re-evaluate. Kept below the smallest allowed
/// `refresh_seconds` so a short interval is actually honoured, not rounded up.
const POLL_INTERVAL: StdDuration = StdDuration::from_secs(15);
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
    let mut last_activity_seen: Option<DateTime<Utc>> = None;

    loop {
        let asked = match commands.recv_timeout(POLL_INTERVAL) {
            Ok(Command::Shutdown) | Err(RecvTimeoutError::Disconnected) => return,
            Ok(Command::Reconfigure(new_config)) => {
                config = *new_config;
                cache = None; // the directory or the toggle may have changed
                false
            }
            Ok(Command::Refresh) => true,
            Err(RecvTimeoutError::Timeout) => false,
        };

        let snapshot = compute(
            &config,
            &mut client,
            &mut cache,
            &mut last_activity_seen,
            asked,
        );
        if snapshots.send(snapshot).is_err() {
            return; // UI is gone
        }
        on_wake();
    }
}

/// Reads the logs (for freshness and as the fallback), then overlays Anthropic's
/// official percentages when the endpoint answers.
///
/// `asked` marks a refresh somebody wanted — the tray menu, or the log watcher seeing
/// a write. That, and any log activity newer than `last_activity_seen`, is what earns
/// this wake an early re-fetch instead of the full `refresh_seconds` wait.
fn compute(
    config: &AppConfig,
    client: &mut OAuthUsageClient,
    cache: &mut Option<(OfficialUsage, DateTime<Utc>)>,
    last_activity_seen: &mut Option<DateTime<Utc>>,
    asked: bool,
) -> UsageSnapshot {
    let now = Utc::now();
    let claude_dir = reader::resolve_claude_dir(&config.claude_dir);

    // Only the block covering `now` is ever reported, and it cannot start earlier
    // than one window ago, so that is as far back as the logs need reading.
    let since = now - calculator::window_span(config.window_hours);
    let read = reader::read_events(&claude_dir, since, |warning| eprintln!("usage: {warning}"));
    let local = calculator::compute(
        &read.events,
        config.token_limit,
        config.window_hours,
        now,
        read.logs_found,
    );

    // An event newer than the last snapshot saw means usage has actually been spent,
    // so the cached percentages are already behind. `None` sorts below every
    // timestamp, so the first reading that contains any activity counts as new.
    let advanced = local.last_activity > *last_activity_seen;
    if advanced {
        *last_activity_seen = local.last_activity;
    }

    if !config.use_official_usage {
        return local;
    }

    let interval = if asked || advanced {
        Duration::seconds(MIN_FETCH_INTERVAL)
    } else {
        Duration::seconds((config.refresh_seconds as i64).max(MIN_FETCH_INTERVAL))
    };

    let Some(official) = official_usage(config, client, cache, now, interval) else {
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

/// Returns the official figures, re-fetching only once `interval` has passed since
/// the last one and serving the last good value while it is still fresh enough.
fn official_usage(
    config: &AppConfig,
    client: &mut OAuthUsageClient,
    cache: &mut Option<(OfficialUsage, DateTime<Utc>)>,
    now: DateTime<Utc>,
    interval: Duration,
) -> Option<OfficialUsage> {
    let age = cache.as_ref().map(|(_, at)| now - *at);

    if let (Some(age), Some((cached, _))) = (age, cache.as_ref()) {
        if age < interval {
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
