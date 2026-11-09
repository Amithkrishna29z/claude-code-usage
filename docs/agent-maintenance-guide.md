# Agent maintenance guide

Use this guide for small, source-backed maintenance changes.

## Start every task this way

1. Read the root README and relevant `docs/` page.
2. Inspect the owning source module before proposing code.
3. Define minimal verifiable success criteria.
4. Run `git status --short`; preserve unrelated user work and never reset/reformat it.
5. Change only requested behavior, its direct tests, and its direct documentation.

## Source-of-truth map

| Need | Start with | Also inspect |
| --- | --- | --- |
| Config field/default | `usage-core/src/models.rs` | `config.rs`, tests, docs. |
| Config safety/path | `usage-core/src/config.rs` | widget persistence/reload. |
| JSONL shape | `usage-core/src/parser.rs` | reader tests, total token definition. |
| Local session math | `usage-core/src/calculator.rs` | monitor cutoff, tests. |
| Log discovery | `usage-core/src/reader.rs` | watcher lifecycle. |
| Endpoint behavior | `usage-core/src/oauth.rs` | monitor cache, tests, docs. |
| Timing/watcher/cache | `usage-app/src/monitor.rs` | pipeline docs. |
| Daily question/schedule/notification | `usage-app/src/question.rs` | config, tray submenu, daily-question docs. |
| Widget state/position | `usage-app/src/widget.rs` | config, styles, tray commands. |
| Face layout | `usage-app/src/styles.rs` | `WidgetStyle`, resize code. |
| Strings/colors | `usage-app/src/visuals.rs` | widget and tray. |
| Menu/tray | `usage-app/src/tray.rs` | widget tray-command match, Linux GTK path. |
| Distribution | `install.sh`, CI workflow | README install/release docs. |

## Preserve these invariants

- Transport endpoint percentages become fractional `UsageWindow.utilization` values.
- Local fallback never claims a weekly value.
- Endpoint errors never crash; current official cache remains usable for 15 minutes.
- The 60-second endpoint floor remains even for user-triggered refresh.
- Local logs are still read when official data is active.
- Corrupt config is never overwritten by a routine UI save.
- Hidden means off-screen parking, not native invisibility; parking must not persist.
- UI frames never block on I/O/network/native tray loops.
- Linux GTK tray objects remain on their owning thread.
- Daily question marks its date only after the CLI returned a non-empty successful
  result; it may retry a failed CLI call after ten minutes.
- Adding a style is multi-file: enum, serialization, menu, size, renderer, docs/tests.

## Change playbooks

### Official endpoint field

Add a sanitized fixture first, parse defensively, decide whether it belongs in
`OfficialUsage` then `UsageSnapshot`, test full/absent/malformed/renamed forms, and
only then render it. Never log tokens or credentials.

### Local-estimation rule

Write a fixed-UTC test first. Keep block boundaries, empty data, expiration, and zero
limit behavior explicit. Revisit monitor's read cutoff if older events can matter.

### UI/tray action

Add a `TrayCommand`, handle it in the single `WidgetApp::update` match, move expensive
work into monitor/background work, define persistence, and account for Linux tray
constraints.

### Daily question behavior

Keep schedule calculation local-time based and preserve one-date-at-a-time behavior
across restart. Execute Claude without a shell (`Command::new("claude")`), keep model
output out of command construction, and keep the CLI call off the UI thread. Update
the scheduler tests, tray presets, persisted config, and [daily-question docs](daily-question.md)
together.

### New style

Update `WidgetStyle::ALL`, key/label/default behavior, `size_of`, renderer, menu,
docs, config tests, corner clearance, and scale-factor behavior.

### Monitor/watcher lifecycle

Specify behavior for missing paths, changed directories, shutdown, and write bursts.
Preserve debounce/fetch safeguards unless requirements explicitly change them. Add pure
policy tests and a real-desktop watcher smoke check.

## Done means

- Every changed line traces to the request.
- Relevant tests plus `cargo fmt --all -- --check`, `cargo clippy --workspace
  --all-targets`, and `cargo test --workspace --all-features` pass where supported.
- No OAuth token, credential content, or real session-log data enters docs/tests/output.
- Root README and this docs set reflect changed user behavior and maintenance rules.
- Unrelated worktree edits remain untouched.
