# UI, tray, and platform behavior

## Widget contract

The widget is a transparent, app-painted card in a frameless native window. It is
non-resizable, excluded from the taskbar, and requests always-on-top. The card owns a
rounded dark background, border, drag surface, and two subtle controls:

- Upper-left dash: parks/minimizes it off-screen; tray restores it.
- Upper-right cross: persists config and quits.

Drag begins only when the pointer did not start in either corner-control hit box.
The update loop requests repaint every 250 ms, keeping relative text live and acting
as a fallback for native tray events that cannot immediately wake the UI.

## Styles

`WidgetStyle` is persisted and listed in menu order. Sizes are logical points.

| Key | Menu label | Size | Content |
| --- | --- | --- | --- |
| `rings` | Rings (default) | 122×142 | Outer weekly ring, inner session ring, session percent, compact footer. |
| `bars` | Bars | 172×88 | Session/weekly progress bars and footer. |
| `pill` | Pill | 186×40 | Session dial, percent, footer. |
| `minimal` | Minimal | 72×78 | Session percent and exceptional caption. |
| `text` | Text | 244×32 | One centered text summary. |

All styles share these rules:

- Session is primary; weekly appears only when known.
- Green is under 70% used, orange is 70% through under 90%, red is 90% or more, grey
  is unknown/inactive.
- `NoActiveSession` may show a grey 0%; `NoData` and `NoLogsFound` show an em dash.
- Local estimates are marked `est.`. Local activity over ten minutes old adds `stale`.
- Durations include seconds below one hour, omit seconds at least one hour, and reset
  times more than a day away show local weekday/time.

Style resize must be re-applied when the monitor scale factor changes. A point size
otherwise becomes a stale physical-pixel pin after moving between differently scaled
displays.

## Tray

The tray is a hand-rasterized 32×32 RGBA icon: ring tracks, optional weekly outer
arc, session inner arc, and session-coloured center. Pixels are rebuilt only when
rounded percentages change; tooltip state is pushed only when its text changes.

```text
Show/hide widget
Refresh now
────────────
Style > Rings / Bars / Pill / Minimal / Text
Daily question > Off / 5 AM / 6 AM / 7 AM / 8 AM / 9 AM / noon / 6 PM / 9 PM
Reload settings
────────────
Quit
```

- **Refresh now** requests monitor work but does not override endpoint fetch floors.
- **Style** uses check items. The complete group is re-ticked inside the native menu
  callback because native auto-toggle can otherwise leave two or zero choices shown.
- **Daily question** is a second check-item group. It sets the configured local time
  or disables the feature with **Off**. Hand-edited valid times outside these presets
  work, but honestly leave every preset unticked.
- **Reload settings** behavior is defined in [Configuration](configuration.md).
- **Quit** persists readable config then closes the native viewport.

On Windows/macOS, left-release on the tray toggles widget visibility. The tray does
not open a menu on left click, avoiding a second queued toggle. Right click opens the
menu. The tray tooltip is the detailed display: percentages/reset times, source,
local estimate token totals, and last local activity. Keep it in
`visuals::detail_text` so widget and tray describe data consistently.

## Platform behavior

### Windows

- Release builds suppress the console window.
- Saved position is validated against connected monitors and recovers to `(100, 100)`
  if the saved monitor disappeared.
- Left-click and tray tooltip are supported.

### macOS

- Uses shared non-Linux main-thread tray implementation.
- Left-click and tooltip are supported.
- Published binaries are unsigned; see the root README for Gatekeeper first-run steps.

### Linux

- The tray is GTK/AppIndicator on a dedicated `tray-gtk` main-loop thread. The UI
  sends simple updates because tray/menu objects are not transferable between threads.
- A StatusNotifier/AppIndicator host is required for the tray; the widget still works
  without one.
- AppIndicator lacks equivalent left-click and tooltip behavior. Use the menu for
  show/hide and the widget for detail.
- If Wayland and `DISPLAY` coexist, the app forces X11/XWayland by default to retain
  always-on-top and position behavior. `CLAUDE_USAGE_BACKEND=wayland` opts out and
  accepts native-Wayland limits.
- X11 uses utility-window type to avoid task lists and stay above normal windows.

## UI change checklist

- Adding a style updates enum, serialization key/label/order, fixed size, renderer,
  menu, docs, and config tests.
- Keep corner glyph clearance in every renderer.
- Keep expensive work out of `eframe::App::update`.
- Smoke-test visible and parked states after viewport/tray changes.
