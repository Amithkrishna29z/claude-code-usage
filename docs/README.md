# Maintainer documentation

This directory is the source-oriented guide for people and coding agents maintaining
Claude Code Usage. It describes the implementation in this checkout. The root
[README](../README.md) is the user-facing install and usage guide; start here when
changing behavior or tracing a bug.

## Read in this order

1. [Architecture](architecture.md) — crates, ownership, threads, and end-to-end flow.
2. [Usage data and refresh pipeline](usage-data-and-refresh.md) — endpoint, local
   fallback, caching, timing, and failure behavior.
3. [Configuration](configuration.md) — persisted schema, defaults, paths, and safe
   configuration changes.
4. [Daily question](daily-question.md) — scheduler, Claude CLI execution,
   notifications, presets, and failure behavior.
5. [UI, tray, and platforms](ui-tray-and-platforms.md) — widget contract, menu, and
   OS differences.
6. [Development and verification](development-and-verification.md) — tests, CI,
   release artifacts, installer, and smoke checks.
7. [Agent maintenance guide](agent-maintenance-guide.md) — source-of-truth map,
   invariants, and change playbooks.

## System at a glance

```text
Claude Code credentials + session JSONL files
                 |
                 v
usage-core: reader -> parser -> calculator ----+--> local UsageSnapshot
                                                |
                                                +--> freshness metadata
                 |
                 v
usage-core: OAuthUsageClient ------------------+--> official UsageWindow values
                                                        |
                                                        v
usage-app monitor worker -> snapshot channel -> egui widget + tray icon/tooltip
```

The user sees a small transparent, always-on-top widget and a tray icon. The app
prefers Anthropic's usage endpoint for the session and weekly percentages. It always
reads local logs as well, both to provide a fallback and to report recent local
activity. No backend service, telemetry service, or database exists.

Separately, the app runs the locally installed Claude CLI once per configured local
day to generate a daily question and posts it through the native notification system.

## Repository map

```text
Cargo.toml                         Workspace definition and release profile
crates/usage-core/                 Pure, headless domain/config/data-source logic
  src/models.rs                    Shared types, widget-style enum, AppConfig
  src/config.rs                    Platform config load/save service
  src/parser.rs                    One JSONL line -> UsageEvent
  src/reader.rs                    Log discovery, cutoff filtering, de-duplication
  src/calculator.rs                Local 5-hour session-block estimate
  src/oauth.rs                     Official endpoint client and defensive parser
  tests/core_tests.rs              Offline core coverage
crates/usage-app/                  Native desktop application
  src/main.rs                      Window creation and Linux backend choice
  src/monitor.rs                   Background refresh worker and file watcher
  src/question.rs                  Daily question scheduling, CLI call, notification
  src/widget.rs                    UI state, persistence, visibility, dragging
  src/styles.rs                    Five widget renderers and sizing contract
  src/tray.rs                      Tray lifecycle, menu, raster icon
  src/visuals.rs                   Shared colors and display formatting
install.sh                         Linux installer/uninstaller
.github/workflows/ci.yml           Lint, test, build, rolling, release workflows
```

## Important boundaries

- `usage-core` must remain GUI-free. Its calculations and parsing are testable without
  a display server or real Claude installation.
- `usage-app` owns native UI, tray integration, process lifetime, background work,
  and platform-specific behavior.
- `UsageSnapshot` is the contract between acquisition and rendering. Prefer extending
  that contract over making the widget reach into reader or OAuth code directly.
- Configuration is hand-editable JSON. Never overwrite a malformed configuration
  file merely because the running app had to use defaults.

## Intentional absences

The app reports Claude Code allowance usage and has a daily-question notification. It
does not collect user answers, retain question history, have a general settings
window, or upload data to a third party. Treat those as new features, not existing
behavior.
