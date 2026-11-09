# Architecture

## Purpose and runtime shape

`usage-app` is a cross-platform Rust desktop application built with `eframe`/`egui`.
It creates one frameless, transparent, non-resizable, always-on-top widget window and
tries to create one system-tray icon. The widget still runs if no tray is available.

`usage-core` is the reusable domain layer. It owns parsing, local estimation,
configuration, OAuth endpoint parsing, and shared types. It has no UI dependency.

The app also has a small independent daily-question path: the UI frame checks its
local-time schedule, starts a background Claude CLI call when due, and delivers the
completed text as a native notification on the UI thread.

The workspace uses Rust 2021, MSRV 1.82, version `0.4.0`, and a size-oriented release
profile: `opt-level = "z"`, LTO, one codegen unit, and stripped binaries.

## Runtime data flow

```text
main()
  |-- ConfigService::load()
  |-- builds fixed-size eframe viewport for configured WidgetStyle
  |-- UsageMonitor::start(config, request_repaint callback)
  `-- WidgetApp owns config + monitor + latest UsageSnapshot

UsageMonitor worker thread (every 15 seconds or on command)
  |-- read Claude logs for local activity and fallback estimate
  |-- calculate local UsageSnapshot
  |-- when enabled, fetch/cache official usage figures
  |-- overlay official session/weekly values when available
  `-- send snapshot over mpsc channel; request UI repaint

egui update loop
  |-- lazily create tray on the main thread
  |-- drain snapshots, drain tray commands
  |-- update tray pixels and tooltip
  |-- tick daily-question scheduler and save successful date marker
  |-- render card when visible
  `-- repaint every 250 ms for relative-time and tray responsiveness
```

The UI never performs filesystem I/O or HTTP itself. The `UsageMonitor` worker owns
that work, so a slow disk or OAuth call cannot stall a UI frame.

## Module ownership

| Module | Responsibility | Keep out of it |
| --- | --- | --- |
| `usage-core/models.rs` | Events, windows, snapshots, states/sources, styles, config. | UI/native APIs. |
| `usage-core/config.rs` | Config paths, load/save, BOM handling, malformed-file safety. | Widget behavior. |
| `usage-core/parser.rs` | Defensive JSONL line -> event conversion. | File traversal/block math. |
| `usage-core/reader.rs` | Claude path, log discovery, cutoff filtering, message-ID de-dupe. | Token percentage math. |
| `usage-core/calculator.rs` | Local session-block grouping and snapshot production. | File or UI work. |
| `usage-core/oauth.rs` | Token lookup, endpoint request/backoff, defensive response parsing. | Rendering. |
| `usage-app/monitor.rs` | Worker, watcher, refresh cadence/cache, snapshot publication. | Widget drawing. |
| `usage-app/question.rs` | Daily schedule check, background `claude -p`, notification, retry. | Usage calculation. |
| `usage-app/widget.rs` | UI state, persistence, tray command application, drag/visibility. | Log/HTTP work. |
| `usage-app/styles.rs` | Style dimensions and drawing. | State persistence. |
| `usage-app/tray.rs` | Menu/event bridge, native tray lifecycle, raster icon. | Usage calculation. |
| `usage-app/visuals.rs` | Color thresholds and display formatting. | Platform integration. |

## Shared domain contract

`UsageSnapshot` is the sole data shape consumed by widget and tray. Its important
members are:

- `state`: `Ok`, `NoActiveSession`, `NoData`, or `NoLogsFound`.
- `source`: `Official` or `LocalLogs`.
- `session` and `weekly`: optional `UsageWindow`s with a fraction consumed and an
  optional UTC reset time.
- local context: `tokens_used`, `token_limit`, `window_start`, `events_counted`.
- freshness context: `last_activity` and `generated_at`.

Official snapshots intentionally retain local activity/token metadata, because the
endpoint does not say whether local Claude Code was used recently. Official data is
used only if it includes the session window. A weekly-only response falls back to the
local result instead of fabricating a session value.

## Threads and lifecycle

- eframe and `WidgetApp` run on the native main/UI thread.
- `UsageMonitor` starts a named `usage-monitor` worker. It owns the endpoint client,
  official cache, and last-activity cursor.
- Worker commands (`Refresh`, `Reconfigure`, `Shutdown`) and snapshots use `mpsc`.
  `latest()` drains old snapshots and uses only the newest.
- The filesystem watcher only sends refresh commands; it never reads logs directly.
- Linux owns tray objects and GTK menu state on a separate `tray-gtk` thread. It
  receives only simple pixel/string/style messages because native objects are not
  `Send`.
- `DailyQuestion` starts a named `daily-question` thread for each generation attempt.
  It returns a `Result<String, String>` to the UI over `mpsc`; notification creation
  stays on the UI thread for macOS compatibility.
- Tray global handlers are installed exactly once because the dependency exposes
  process-wide handler registration.

Startup loads config, creates the fixed viewport, applies the Linux backend choice,
starts the worker, then creates the tray in the first UI frame. Closing persists
readable config and closes the viewport; dropping the monitor sends shutdown.

## Invariants

- All model timestamps are UTC; local time is used only at display time.
- `UsageWindow.utilization` is a fraction (`0.27`), though the endpoint sends a
  percentage (`27.0`).
- Local fallback never invents weekly usage.
- Local estimates must be labelled `est.` when a face has room.
- Five style sizes are a contract between `styles::size_of`, window startup, and
  dynamic style resizing.
