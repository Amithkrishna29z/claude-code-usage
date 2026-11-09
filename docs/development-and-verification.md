# Development and verification

## Commands

Requires stable Rust 1.82+. Live behavior also needs Claude Code signed in, but tests
need neither Claude logs nor network. Linux builds need the GUI development libraries
listed in the root README.

```powershell
cargo run -p usage-app
cargo fmt --all -- --check
cargo clippy --workspace --all-targets
cargo test --workspace --all-features
cargo test -p usage-core
cargo build --release -p usage-app
```

Debug builds on Windows may have a console; release builds intentionally suppress it.
Do not expect native tray/window behavior in a headless test process.

## Tests

`crates/usage-core/tests/core_tests.rs` is offline coverage for JSONL parsing, all
token fields, block gaps/expiration, zero limits, missing logs, de-duplication, cutoff
filtering, Claude path resolution, OAuth response parsing/credentials/backoff, and
config load/save/BOM/typo/corruption behavior.

`usage-app/src/visuals.rs` has focused tests for time, token, and color formatting.
`usage-app/src/question.rs` has scheduler tests for before/after time, one run per
date, off mode, invalid-time fallback, and menu presets. Native UI/tray behavior is
built in CI but must be smoke-tested on an actual desktop.

## CI and releases

`.github/workflows/ci.yml` sets `RUSTFLAGS=-D warnings` and runs:

| Job | Verification |
| --- | --- |
| `lint` on Ubuntu | Formatting plus workspace/all-target clippy, with Linux GUI deps. |
| `test` on Ubuntu/Windows/macOS | All-feature workspace tests, then workspace build. |
| `build` | Release artifacts for Windows x86_64, macOS arm64/x86_64, Linux x86_64. |
| `rolling` | Replaces `latest` prerelease after successful `main` build. |
| `release` | Publishes artifacts when a `v*` tag is pushed. |

Artifact names are `claude-usage-windows-x86_64` (plus `.exe`),
`claude-usage-macos-aarch64`, `claude-usage-macos-x86_64`, and
`claude-usage-linux-x86_64`.

## Linux installer

`install.sh` is Linux x86_64-only. It supports `--nightly`, `--no-autostart`, and
`--uninstall`; checks the downloaded file's ELF magic before installation; installs
to `~/.local/bin/claude-usage` by default; prompts before installing missing runtime
libraries; creates menu/autostart entries; and leaves user config intact on uninstall.

Treat installer changes as distribution/security-sensitive. Preserve interactive
confirmation and test install/uninstall in a disposable environment.

## Manual smoke matrix

| Area | Check |
| --- | --- |
| Core data logic | Add deterministic fixture/timestamp tests and run core suite. |
| OAuth/cache | Verify fallback without credentials/logs and live official data without exposing a token. |
| Daily question | Use a time a few minutes ahead; verify one Claude notification, persistence across restart, Off mode, and CLI failure/retry behavior. |
| Config | Edit JSON, Reload settings, restart, then confirm malformed file is not overwritten. |
| Widget | Move, minimize, restore, close/relaunch; test affected styles. |
| Tray | Exercise every changed menu action, radio tick behavior, icon, and supported click behavior. |
| Windows position | Save on secondary screen, disconnect/rearrange it, confirm recovery. |
| Linux | Test actual AppIndicator host and relevant X11/XWayland/Wayland configuration. |
| Packaging | Confirm target artifact names and installer paths/desktop entries. |

Update the root README for user behavior and `docs/` for design/agent context in the
same feature change.
