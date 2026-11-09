# Configuration

## Location

`ConfigService` writes `config.json` below `ClaudeCodeUsage` in the platform config
directory:

| Platform | Path |
| --- | --- |
| Windows | `%APPDATA%\\ClaudeCodeUsage\\config.json` |
| macOS | `~/Library/Application Support/ClaudeCodeUsage/config.json` |
| Linux | `~/.config/ClaudeCodeUsage/config.json` |

If no platform directory is available, it falls back to `./ClaudeCodeUsage`.
`ConfigService::with_dir` exists for tests.

## Current schema

```json
{
  "use_official_usage": true,
  "token_limit": 20000000,
  "window_hours": 5.0,
  "refresh_seconds": 300,
  "claude_dir": "",
  "widget_left": 1144.0,
  "widget_top": 602.0,
  "widget_visible": true,
  "style": "rings",
  "daily_question_time": "06:00",
  "daily_question_last_asked": ""
}
```

| Key | Type/default | Meaning |
| --- | --- | --- |
| `use_official_usage` | bool / `true` | Enables OAuth usage; `false` is local/offline only. |
| `token_limit` | i64 / `20_000_000` | Local-estimate denominator only. |
| `window_hours` | f64 / `5.0` | Local session duration; non-positive is treated as 5.0. |
| `refresh_seconds` | u64 / `300` | Idle official interval; actual minimum is 60 seconds. |
| `claude_dir` | string / `""` | Empty/whitespace means `~/.claude`. |
| `widget_left`, `widget_top` | optional f32 | Saved logical-point position. |
| `widget_visible` | bool / `true` | Start visible or parked off-screen. |
| `style` | enum / `"rings"` | `rings`, `bars`, `pill`, `minimal`, or `text`; unknown becomes `rings`. |
| `daily_question_time` | string / `"06:00"` | Local 24-hour `HH:MM` time. Empty disables questions; unreadable non-empty values run at 06:00. |
| `daily_question_last_asked` | string / `""` | App-managed local date `YYYY-MM-DD` of last successful daily question. |

Unknown keys are ignored and missing keys get defaults, preserving forward and
backward compatibility.

## Read/write safety

- Missing config yields `AppConfig::default()`.
- A leading UTF-8 BOM is removed before JSON parsing.
- Invalid JSON logs an error and uses defaults for this run, but leaves the file
  untouched.
- Before UI persistence, `WidgetApp::persist` calls `is_readable`; a malformed file
  is never overwritten with in-memory defaults.
- Saves create the parent directory and write pretty JSON.
- Style, visibility, and settled drag position persist immediately.
- A shown daily question writes `daily_question_last_asked`, preventing a second
  question after restart on the same local date.
- **Reload settings** reads from disk without restart. It retains current live
  position/visibility (the file may be stale) but intentionally applies a new style.

## Position and visibility

Hiding means moving to `(-32000, -32000)`, not setting native visibility false. A
truly invisible eframe viewport stops update calls and tray polling. Coordinates at
or below `-10000` are treated as parked/off-screen and are never saved.

Windows validates the saved logical point against connected displays via
`MonitorFromPoint`; an old location on a disconnected monitor falls back to `(100,
100)`. Other platforms accept stored positions because this layer cannot query their
full multi-monitor layout.

## Adding or changing a setting

1. Add a serde default and `Default` value in `usage-core/src/models.rs`.
2. Validate nonsensical values at their use boundary.
3. Thread reload through `WidgetApp::reload_settings` and monitor reconfiguration if
   background behavior changes.
4. Add round-trip, missing-key, corrupt-file, and invalid-value tests.
5. Update this file and the root README configuration section in the same change.

Do not rename a persisted key silently. Implement backwards-compatible deserialization
and document a migration if a rename is necessary.
