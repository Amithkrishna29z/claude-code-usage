# Claude Code Usage — cross-platform desktop widget

A tray app + always-on-top mini widget showing how much of your Claude Code allowance
is gone: the **5-hour session window** and the **7-day weekly window**, as two
concentric rings. Written in Rust, builds for **Windows, macOS, and Linux**.

The numbers come from **Anthropic's own usage endpoint** — the same figures Claude
Code's `/usage` and the Usage panel on claude.ai show — so they need no calibration.
If that endpoint is unavailable, the app falls back to an estimate derived from your
local Claude Code session logs.

---

## What it shows

**Mini widget** (always-on-top, frameless, draggable) — deliberately minimal:

- **Concentric rings**: outer = 5-hour session, inner = 7-day weekly. Each is coloured
  independently: **green < 70%**, **orange 70–90%**, **red > 90%**.
- The session percent in the centre, since that is the limit that usually bites first.
- One line beneath: `wk 54%  ·  2h 20m left`.
- A caption appears under the percent only when something needs flagging — `est.` on
  the local fallback, `stale` after 10 minutes of no activity. It stays blank when the
  numbers are official and current.
- Remembers its position between runs.

**System tray icon**

- The same two rings, rasterised at 32×32.
- Hovering shows the full detail: both percentages with reset times, whether the
  figures are official or a local estimate, and last activity.
- **Left-click** toggles the widget.
- **Menu**: Show/hide widget, Refresh now, Reload settings, Quit.

---

## Install

Prebuilt binaries for all three platforms are attached to every release:

- **[Latest release](https://github.com/Amithkrishna29z/claude-code-usage/releases/latest)** — a fixed, tagged build.
- **[`latest` prerelease](https://github.com/Amithkrishna29z/claude-code-usage/releases/tag/latest)** — rebuilt on every push to `main`, so its contents change over time.

| Platform | Asset |
|---|---|
| Windows 10/11 (x64) | `claude-usage-windows-x86_64.exe` |
| macOS (Apple silicon) | `claude-usage-macos-aarch64` |
| macOS (Intel) | `claude-usage-macos-x86_64` |
| Linux (x86-64, glibc) | `claude-usage-linux-x86_64` |

There is no installer and no main window: the download *is* the app, and it puts an
icon in the tray/menu bar. Put it wherever you keep local binaries and run it.

It also needs **Claude Code installed and signed in on the same machine** — that is
where both the OAuth token and the session logs come from. Without `~/.claude`, the
widget has nothing to read.

The binaries are **unsigned**, so each OS pushes back the first time in its own way;
the steps below are how you get past that.

### Windows

```powershell
# from wherever you saved it
.\claude-usage-windows-x86_64.exe
```

SmartScreen will show "Windows protected your PC" — choose **More info → Run anyway**.
(Or clear the download flag first: `Unblock-File .\claude-usage-windows-x86_64.exe`.)

The tray icon usually starts hidden under the **"^"** overflow arrow; drag it onto the
taskbar to keep it visible. Left-click it to show the widget.

To start it with Windows, press `Win+R`, run `shell:startup`, and drop a shortcut to
the exe in the folder that opens.

### macOS

```bash
# Apple silicon; swap in the x86_64 asset on an Intel Mac
chmod +x claude-usage-macos-aarch64
xattr -d com.apple.quarantine claude-usage-macos-aarch64
./claude-usage-macos-aarch64
```

Without the `xattr` step, Gatekeeper refuses to open it ("cannot be opened because the
developer cannot be verified"). If you skip it and get blocked anyway, **System
Settings → Privacy & Security → Open Anyway** allows that one binary.

The icon appears in the **menu bar**, top-right. Left-click toggles the widget.

To keep it around, move it somewhere stable (`/usr/local/bin`, say). It is a plain
binary rather than an `.app` bundle, so for login startup the reliable route is a
LaunchAgent — a `~/Library/LaunchAgents/com.local.claude-usage.plist` with
`ProgramArguments` pointing at the binary and `RunAtLoad` set, loaded with
`launchctl load`.

### Linux

The binary is dynamically linked against GTK and the appindicator tray, so install the
runtime libraries first:

```bash
# Debian / Ubuntu / Linux Mint
sudo apt-get install -y libgtk-3-0 libayatana-appindicator3-1 libxdo3

# Fedora
sudo dnf install gtk3 libappindicator-gtk3 xdotool

# Arch
sudo pacman -S gtk3 libayatana-appindicator xdotool
```

```bash
chmod +x claude-usage-linux-x86_64
./claude-usage-linux-x86_64
```

The tray icon needs a StatusNotifier host: Cinnamon, MATE, Xfce and KDE Plasma all
provide one, as does GNOME with the AppIndicator extension; a bare GNOME Shell shows no
icon, in which case the widget itself is still usable. Under Wayland the widget's
always-on-top and remembered position depend on the compositor.

Two tray behaviours differ here, because the appindicator protocol has no equivalent:
**left-click does not toggle the widget** — use the menu's *Show/hide widget* — and the
**hover tooltip with the full readout is absent**, so the widget itself is where the
numbers live.

#### Linux Mint

Mint is Ubuntu-based (LMDE is Debian-based), so the `apt-get` line above is the right
one, and Cinnamon's system tray shows appindicator icons without extra setup. End to
end, from the downloaded file:

```bash
sudo apt-get install -y libgtk-3-0 libayatana-appindicator3-1 libxdo3

mkdir -p ~/.local/bin
mv ~/Downloads/claude-usage-linux-x86_64 ~/.local/bin/claude-usage
chmod +x ~/.local/bin/claude-usage
~/.local/bin/claude-usage
```

On Mint 22 (Ubuntu 24.04 base) `libgtk-3-0` is a transitional package and apt pulls in
`libgtk-3-0t64` instead — that is expected, not an error. Mint's default session is
X11, so always-on-top and the remembered widget position behave as on Windows.

The icon lands in the panel's system tray, bottom-right; left-click it to toggle the
widget. To start it at login, use **Menu → Startup Applications → Add → Custom
command** and point it at `~/.local/bin/claude-usage` (that GUI writes the same
`~/.config/autostart` entry shown below).

#### Autostart on other desktops

Drop a `.desktop` file in `~/.config/autostart/`:

```ini
[Desktop Entry]
Type=Application
Name=Claude Code Usage
Exec=/home/you/.local/bin/claude-usage-linux-x86_64
X-GNOME-Autostart-enabled=true
```

---

## Build from source

Requires a stable Rust toolchain (1.82+).

```bash
git clone https://github.com/Amithkrishna29z/claude-code-usage.git
cd claude-code-usage
cargo run --release -p usage-app
```

The binary is ~5 MB and lands at `target/release/usage-app` (`.exe` on Windows).
There is no main window — look for the tray icon and left-click it to show the widget.

### Linux build dependencies

Building (as opposed to running) needs the development headers:

```bash
sudo apt-get install -y \
  libgtk-3-dev libxdo-dev libayatana-appindicator3-dev \
  libx11-dev libxcursor-dev libxrandr-dev libxi-dev \
  libgl1-mesa-dev libwayland-dev libxkbcommon-dev
```

### Tests

```bash
cargo test -p usage-core
```

All logic lives in `usage-core`, which is headless, so the suite runs fully offline —
no `.claude` logs, no network, no display server.

---

## Where the numbers come from

### Primary: Anthropic's usage endpoint

`GET https://api.anthropic.com/api/oauth/usage`, authenticated with the OAuth access
token Claude Code stores in `~/.claude/.credentials.json`. This is exactly what
Claude Code's `/usage` does. The fields read:

```json
{ "five_hour": { "utilization": 27.0, "resets_at": "2026-09-16T19:50:00+00:00" },
  "seven_day": { "utilization": 48.0, "resets_at": "2026-09-20T21:59:59+00:00" } }
```

`utilization` is a percentage of that window's allowance; `resets_at` is when it rolls
over. Everything else in the response is ignored.

**Two things to know about this endpoint:**

1. **It is internal and undocumented.** It can change or disappear without notice.
   Every field is read defensively, and any failure — missing credentials, an expired
   token, a network error, a renamed field — falls back to the local estimate rather
   than showing a wrong number. If Anthropic changes the shape, update `read_window`
   in [`crates/usage-core/src/oauth.rs`](crates/usage-core/src/oauth.rs).
2. **It rate-limits hard.** It answers `429` with a `Retry-After` of a few minutes.
   The app fetches at most once every 5 minutes, honours `Retry-After` when told to
   back off, and keeps serving the last good figures for up to 15 minutes so a single
   blip does not flip the widget over to the estimate. Five-minute resolution is ample
   for a 5-hour and a 7-day window.

The token is only ever sent to `api.anthropic.com`. Set `use_official_usage` to
`false` to disable all network access and use the local estimate only.

### Fallback: local session logs

When the endpoint is unavailable, the app derives a 5-hour figure from
`~/.claude/projects/**/*.jsonl`:

1. **Read** every `*.jsonl` under `{claude_dir}/projects` recursively, streaming and
   read-only so it never blocks Claude Code from writing.
2. **Parse** each line as JSON and keep the ones carrying a `message.usage` block.
   Duplicate assistant messages (same `message.id`, e.g. after a session resume) are
   counted once.
3. **Total tokens per event** = `input_tokens + output_tokens +
   cache_creation_input_tokens + cache_read_input_tokens`.
4. **Group into 5-hour session blocks.** A block starts at its first event and lasts
   `window_hours`. A new block starts when an event is more than the window after the
   block's start, **or** more than the window after the previous event (the ">5h gap"
   rule). The active block is the one whose window still covers *now*.
5. If the most recent block's window has fully elapsed with no new activity, the
   widget shows **no session** (0%).

This path cannot know your weekly usage, so the inner ring is absent and the `wk`
half of the compact line is dropped while the fallback is in use.

**Expect the fallback and the official figure to disagree.** The fallback counts raw
tokens against a limit you guessed; Anthropic's number reflects your actual plan
allowance. If the widget shows a percentage that does not match `/usage`, check for
the `est.` caption — that means you are looking at the guess, not the real figure.

The logs are read even when the official figures are available, because they supply
the "last activity" freshness that the endpoint does not provide.

---

## Configuration

Settings are a JSON file, edited by hand. Its location follows platform convention:

| Platform | Path |
|---|---|
| Windows | `%APPDATA%\ClaudeCodeUsage\config.json` |
| macOS | `~/Library/Application Support/ClaudeCodeUsage/config.json` |
| Linux | `~/.config/ClaudeCodeUsage/config.json` |

```json
{
  "use_official_usage": true,
  "token_limit": 20000000,
  "window_hours": 5.0,
  "refresh_seconds": 300,
  "claude_dir": "",
  "widget_left": 1144.0,
  "widget_top": 602.0,
  "widget_visible": true
}
```

| Key | Meaning | Default |
|---|---|---|
| `use_official_usage` | Fetch real percentages from Anthropic. `false` = fully offline. | `true` |
| `token_limit` | Per-window token budget for the *fallback estimate only*. | `20000000` (placeholder) |
| `window_hours` | Length of the rolling window used by the fallback. | `5.0` |
| `refresh_seconds` | How often the official figures are re-fetched. Floored at 60 — see below. | `300` |
| `claude_dir` | Root containing `projects/` and `.credentials.json`. Empty = `~/.claude`. | `""` |
| `widget_left` / `widget_top` | Remembered position; managed by the app. | unset |
| `widget_visible` | Whether the widget was showing at exit. | `true` |

Unknown keys are ignored and missing keys fall back to defaults, so the file survives
version changes in both directions. After editing, pick **Reload settings** from the
tray menu — no restart needed.

### How often it refreshes
The countdown on the widget ticks **every second**, and the local estimate updates
within ~1.5s of Claude Code writing a log line.

The official percentages are another matter: that endpoint answers `429` with a
`Retry-After` of a few minutes, so `refresh_seconds` is floored at **60** no matter
what you set. Polling harder does not get fresher numbers — it gets you rate-limited
into the local estimate, which is strictly worse than a slightly stale real figure.
The default of 300 is comfortable; 60 is the floor if you want it tighter. A 5-hour
and a 7-day window simply do not move fast enough for second-by-second polling to
tell you anything.

### Calibrating `token_limit`
This matters only when the official figures are unavailable. Anthropic does not
publish the exact token budget of the 5-hour window, and the app counts **all four**
token components, where cache-read tokens usually dominate — so `20,000,000` is a
placeholder. Watch the widget's fallback number over a busy window and set
`token_limit` a bit above your realistic peak.

### Changing what counts as a "token"
Edit the single `total_tokens` definition in
[`crates/usage-core/src/models.rs`](crates/usage-core/src/models.rs) — everything
downstream uses that one method.

---

## CI/CD

[`.github/workflows/ci.yml`](.github/workflows/ci.yml) runs on every push and pull
request:

| Job | What it does |
|---|---|
| **lint** | `cargo fmt --check` and `cargo clippy`, with `-D warnings` |
| **test** | Core test suite on Ubuntu, Windows, and macOS; full workspace build on each |
| **build** | Release binaries for `x86_64-pc-windows-msvc`, `aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-unknown-linux-gnu`, uploaded as artifacts |
| **rolling** | On every push to `main`, replaces the `latest` prerelease with the fresh binaries |
| **release** | On a `v*` tag, collects those binaries into a GitHub Release |

Cutting a release:

```bash
git tag v0.1.0 && git push origin v0.1.0
```

---

## Project layout

```
Cargo.toml                          # workspace
crates/
  usage-core/                       # pure, testable logic — no UI, no platform code
    src/models.rs                   # UsageEvent, UsageWindow, UsageSnapshot, AppConfig
    src/oauth.rs                    # official endpoint client + defensive parse + backoff
    src/parser.rs                   # JSONL line -> UsageEvent
    src/reader.rs                   # enumerate + de-dupe logs
    src/calculator.rs               # 5-hour session-block math (fallback)
    src/config.rs                   # load/save config.json
    tests/core_tests.rs             # 24 tests, fully offline
  usage-app/                        # GUI (egui/eframe) + tray (tray-icon)
    src/main.rs                     # window setup, entry point
    src/widget.rs                   # the minimal card: rings, percent, compact line
    src/tray.rs                     # tray icon rasteriser, menu, tooltip
    src/monitor.rs                  # worker thread: official-first refresh, cache, watcher
    src/visuals.rs                  # shared colours + formatting
.github/workflows/ci.yml
```

`usage-core` has no GUI dependency, which is what lets the whole test suite run
headless in CI on all three platforms.

---

## Notes & limitations

- **Verified on Windows only.** The app was built and exercised on Windows 11: widget
  rendering, always-on-top, the tray menu, config persistence, the live endpoint, the
  rate-limit backoff, and all three colour tiers. macOS and Linux are **compile- and
  test-verified through CI**, not run by hand — the tray in particular behaves
  differently per platform and deserves a real smoke test before you trust it there.
- **No settings dialog.** The original C# build had one; this port uses the JSON file
  plus **Reload settings** instead. Say the word if you want the dialog back.
- **Left-click and tooltips are Windows/macOS only.** The Linux tray speaks the
  appindicator protocol, which reports menu activations and nothing else: no click
  events, no tooltip. The menu carries every command, and GTK gets a thread of its own
  there because `tray-icon` needs a GTK main loop that eframe's event loop is not.
- **Detail lives on the tray icon**, not the widget. An in-widget hover tooltip gets
  clipped by the 122×142 window, so the full readout moved to the tray tooltip, which
  is a native OS window and cannot clip.
- **No "Start with Windows" toggle.** The C# build wrote an `HKCU\...\Run` key; that
  is Windows-only and was dropped rather than faked cross-platform. Use your
  platform's normal startup mechanism.
- The official figures depend on an **undocumented endpoint** that may change without
  notice; the app degrades to the local estimate rather than breaking.
- The app **is not fully offline by default** — it sends your existing OAuth token to
  `api.anthropic.com` to read your own usage. Set `use_official_usage: false` to
  restore fully-local behaviour.
- The fallback estimate cannot see weekly usage, and its percentage is only as good as
  the `token_limit` you calibrate.
- No telemetry, no third-party servers, no secrets stored by this app.
