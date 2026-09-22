//! The always-on-top mini widget: the card, its window, and the two glyphs in its top
//! corners. What the card actually shows is a [`crate::styles`] face, chosen from the
//! tray. Everything the face leaves out (both reset times, the data source,
//! freshness) lives in the hover tooltip, so the resting state stays quiet without
//! losing information.

use chrono::Utc;
use eframe::egui::{self, Color32, Sense, Stroke, Vec2, ViewportCommand};
use usage_core::{AppConfig, ConfigService, UsageSnapshot, UsageState, WidgetStyle};

use crate::monitor::UsageMonitor;
use crate::styles;
use crate::tray::{Tray, TrayCommand};
use crate::visuals;

/// Where the widget parks when "hidden".
///
/// It is deliberately moved off-screen rather than hidden with
/// `ViewportCommand::Visible(false)`: an invisible window stops receiving redraws
/// entirely, which stalls `update()` — and with it the tray polling — so the widget
/// could never be brought back. Parked off-screen the event loop keeps running and
/// the tray stays responsive.
pub const PARKED: egui::Pos2 = egui::pos2(-32000.0, -32000.0);

/// Fallback placement when showing a widget that has no remembered position.
const DEFAULT_POSITION: egui::Pos2 = egui::pos2(100.0, 100.0);

/// A reported position at or beyond this is the park, not somewhere the user put the
/// window, and must never be remembered as one.
///
/// It is a threshold rather than a comparison against [`PARKED`] itself because that
/// value does not survive the round trip. The move is requested in logical points,
/// Windows clamps the physical result to `i16::MIN`, and egui reads it back divided by
/// the scale factor — so on a 150% display the park reports as -21845, nowhere near
/// the -32000 that was asked for. Matching the sentinel exactly meant the park was
/// saved as the remembered position, and the widget could never be shown again.
const OFF_SCREEN: f32 = -10_000.0;

/// Edge length of the minimise/close hit areas in the card's top corners. Small
/// enough to sit in the gap between the outer ring and the card's corner.
const CORNER_BUTTON: f32 = 14.0;

/// Cards shorter than this are a single row, so the corner glyphs centre themselves
/// against that row instead of hugging the top edge.
const SHORT_CARD: f32 = 60.0;

/// The two buttons in the card's top corners. Minimise parks the widget, which the
/// tray icon brings back; close quits the app outright.
#[derive(Clone, Copy, PartialEq, Eq)]
enum CornerButton {
    Minimise,
    Close,
}

pub struct WidgetApp {
    config: AppConfig,
    config_service: ConfigService,
    monitor: UsageMonitor,
    snapshot: UsageSnapshot,
    /// Created lazily on the first frame: the tray must be built on the main thread.
    tray: Option<Tray>,
    tray_attempted: bool,
    visible: bool,
    /// The face the window is currently sized for. `config.style` is the wish;
    /// this is what has actually been applied, so a change from any source — the
    /// tray menu, a hand-edited config.json — is noticed in one place.
    applied_style: WidgetStyle,
}

impl WidgetApp {
    pub fn new(config: AppConfig, config_service: ConfigService, monitor: UsageMonitor) -> Self {
        let visible = config.widget_visible;
        // main() built the window at this style's size, so it is already applied.
        let config_style = config.style;
        Self {
            snapshot: UsageSnapshot::empty(UsageState::NoData, config.token_limit, Utc::now()),
            config,
            config_service,
            monitor,
            tray: None,
            tray_attempted: false,
            visible,
            applied_style: config_style,
        }
    }

    fn persist(&self) {
        // If the file on disk is malformed we are running on defaults, and writing
        // would erase whatever the user was trying to set. Leave it for them to fix.
        if !self.config_service.is_readable() {
            return;
        }
        if let Err(err) = self.config_service.save(&self.config) {
            eprintln!("usage: could not save config: {err}");
        }
    }

    /// Re-reads config.json so hand-edits take effect without a restart. The
    /// remembered window position is kept from the live config rather than the file,
    /// which may be stale if the widget has been dragged since.
    fn reload_settings(&mut self) {
        let mut reloaded = self.config_service.load();
        reloaded.widget_left = self.config.widget_left;
        reloaded.widget_top = self.config.widget_top;
        reloaded.widget_visible = self.config.widget_visible;
        // The style is deliberately NOT carried over: re-reading the file is how a
        // hand-edited style takes effect, and update() resizes the window to match.
        self.config = reloaded;
        self.monitor.reconfigure(self.config.clone());
    }

    fn toggle_widget(&mut self, ctx: &egui::Context) {
        self.set_visible(ctx, !self.visible);
    }

    /// Parks the window off-screen or brings it back to its remembered spot. See
    /// [`PARKED`] for why this is a move rather than a real hide.
    fn set_visible(&mut self, ctx: &egui::Context, visible: bool) {
        self.visible = visible;
        self.config.widget_visible = visible;

        if visible {
            ctx.send_viewport_cmd(ViewportCommand::OuterPosition(self.shown_position(ctx)));
            ctx.send_viewport_cmd(ViewportCommand::Focus);
        } else {
            ctx.send_viewport_cmd(ViewportCommand::OuterPosition(PARKED));
        }

        self.persist();
    }

    /// Switches the card's face. Persisted immediately, so the choice survives a
    /// restart the same way the position does; the resize happens in `update()`.
    fn set_style(&mut self, style: WidgetStyle) {
        // Re-tick the group on every pick, before anything else decides there is
        // nothing to do. A check item toggles itself when clicked, so the menu is
        // briefly wrong no matter which item was hit: two ticked when the pick is a
        // change, none at all when it is the style already in use. Neither corrects
        // itself, because the early return below never reaches the menu.
        if let Some(tray) = self.tray.as_ref() {
            tray.set_style(style);
        }

        if self.config.style == style {
            return;
        }
        self.config.style = style;
        self.persist();
    }

    /// Resizes the window to the current face and tells the tray which item to tick.
    ///
    /// The viewport was built with its minimum and maximum inner size pinned together
    /// to keep the card unresizable, so the pair has to be relaxed before the new size
    /// will take, then pinned again around it.
    fn apply_style(&mut self, ctx: &egui::Context) {
        let style = self.config.style;
        self.applied_style = style;

        let size = styles::size_of(style);
        ctx.send_viewport_cmd(ViewportCommand::MinInnerSize(Vec2::ZERO));
        ctx.send_viewport_cmd(ViewportCommand::MaxInnerSize(Vec2::INFINITY));
        ctx.send_viewport_cmd(ViewportCommand::InnerSize(size));
        ctx.send_viewport_cmd(ViewportCommand::MinInnerSize(size));
        ctx.send_viewport_cmd(ViewportCommand::MaxInnerSize(size));
        // This frame draws the new face at the old size; the frame that draws it at
        // the new one is only scheduled by the resize coming back round as an event.
        // Ask for it directly instead, so the card is never left mid-change waiting
        // for the window manager to say something.
        ctx.request_repaint();

        if let Some(tray) = self.tray.as_ref() {
            tray.set_style(style);
        }
    }

    /// Where the card should appear.
    ///
    /// A remembered position is only honoured while it still lands on a connected
    /// display. Unplug the monitor the widget was left on — or rearrange the desktop
    /// so those coordinates fall outside every screen — and the position that comes
    /// back from config.json points nowhere. Showing the widget there leaves it
    /// off-desktop with the tray icon as the only sign it is running, and every later
    /// show repeats the same move, so it can never be recovered. Fall back to the
    /// default rather than strand it.
    fn shown_position(&self, ctx: &egui::Context) -> egui::Pos2 {
        let (Some(x), Some(y)) = (self.config.widget_left, self.config.widget_top) else {
            return DEFAULT_POSITION;
        };

        // The remembered position is in points; screens are laid out in pixels.
        let scale = ctx.pixels_per_point();
        if on_a_screen(egui::pos2(x * scale, y * scale)) {
            egui::pos2(x, y)
        } else {
            DEFAULT_POSITION
        }
    }
}

impl eframe::App for WidgetApp {
    /// Transparent so the rounded card can have soft edges rather than sitting in a
    /// rectangle of background colour.
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // The tray needs the main thread, which is where update() runs.
        if !self.tray_attempted {
            self.tray_attempted = true;
            let waker = ctx.clone();
            self.tray = Tray::new(self.config.style, move || waker.request_repaint());
            if self.tray.is_none() {
                eprintln!("usage: no system tray available; widget runs standalone.");
            }
            // main() placed the window from the same remembered position, but with
            // no way to tell whether it still lands on a screen. Re-assert it now
            // that it can be checked, so a widget left on a monitor that is gone is
            // recovered rather than created out of sight.
            let position = if self.visible {
                self.shown_position(ctx)
            } else {
                PARKED
            };
            ctx.send_viewport_cmd(ViewportCommand::OuterPosition(position));
        }

        if let Some(latest) = self.monitor.latest() {
            self.snapshot = latest;
        }

        while let Some(command) = self.tray.as_ref().and_then(Tray::poll) {
            match command {
                TrayCommand::ToggleWidget => self.toggle_widget(ctx),
                TrayCommand::Refresh => self.monitor.refresh_now(),
                TrayCommand::ReloadSettings => self.reload_settings(),
                TrayCommand::SetStyle(style) => self.set_style(style),
                TrayCommand::Quit => {
                    self.persist();
                    ctx.send_viewport_cmd(ViewportCommand::Close);
                }
            }
            // Every one of these changes what the card shows. None of them should
            // wait for the idle tick to come round.
            ctx.request_repaint();
        }

        if self.config.style != self.applied_style {
            self.apply_style(ctx);
        }

        let now = Utc::now();
        if let Some(tray) = self.tray.as_mut() {
            tray.update(&self.snapshot, now);
        }

        if self.visible {
            self.draw_card(ctx, now);
        }

        // Keep relative times live while the window is hidden as well as shown.
        //
        // The countdown is never displayed finer than a second, so this is not about
        // the display: it is the floor under everything that arrives from outside the
        // event loop. A tray click normally wakes the loop through the handler
        // installed with the icon and is acted on in about 15ms, but the click is
        // delivered from inside the menu's own modal loop, and a wake raised there can
        // be missed. When that happens this tick is what catches it, so it sets the
        // worst case a style change can take — a second of apparently nothing, which
        // is far more jarring than the steady cost of a quarter-second tick that now
        // does almost no work.
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
    }
}

impl WidgetApp {
    fn draw_card(&mut self, ctx: &egui::Context, now: chrono::DateTime<Utc>) {
        let frame = egui::Frame::NONE
            .fill(Color32::from_rgba_unmultiplied(0x1E, 0x1E, 0x1E, 0xEE))
            .stroke(Stroke::new(
                1.0_f32,
                Color32::from_rgba_unmultiplied(255, 255, 255, 0x33),
            ))
            .corner_radius(10.0)
            .inner_margin(egui::Margin::symmetric(7, 6));

        let pressed = egui::CentralPanel::default()
            .frame(frame)
            .show(ctx, |ui| {
                let card = ui.max_rect();
                let (minimise_rect, close_rect) = corner_rects(card);

                let response = ui.interact(card, ui.id().with("drag"), Sense::click_and_drag());
                // A press that begins on a corner button belongs to that button:
                // without this the card would slide out from under a click meant to
                // close it.
                let from_corner = response
                    .interact_pointer_pos()
                    .is_some_and(|pos| minimise_rect.contains(pos) || close_rect.contains(pos));
                if response.drag_started() && !from_corner {
                    ctx.send_viewport_cmd(ViewportCommand::StartDrag);
                }

                styles::draw(self.config.style, ui, &self.snapshot, now);

                // Added last so they take the click ahead of the drag surface below.
                let minimise = corner_button(ui, minimise_rect, CornerButton::Minimise).clicked();
                let close = corner_button(ui, close_rect, CornerButton::Close).clicked();

                if close {
                    Some(CornerButton::Close)
                } else if minimise {
                    Some(CornerButton::Minimise)
                } else {
                    None
                }
            })
            .inner;

        match pressed {
            Some(CornerButton::Minimise) => self.set_visible(ctx, false),
            Some(CornerButton::Close) => {
                self.persist();
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
            None => {}
        }

        self.remember_position(ctx);
    }

    /// Records the window position after a drag so it comes back where it was left.
    fn remember_position(&mut self, ctx: &egui::Context) {
        // While parked the reported position is the off-screen one; recording it
        // would lose the real spot and strand the widget when it is shown again.
        if !self.visible {
            return;
        }

        let Some(outer) = ctx.input(|i| i.viewport().outer_rect) else {
            return;
        };
        let (left, top) = (outer.min.x, outer.min.y);
        if left <= OFF_SCREEN || top <= OFF_SCREEN {
            return;
        }

        let moved = self.config.widget_left != Some(left) || self.config.widget_top != Some(top);
        let settled = !ctx.input(|i| i.pointer.any_down());

        if moved && settled {
            self.config.widget_left = Some(left);
            self.config.widget_top = Some(top);
            self.persist();
        }
    }
}

/// The two glyph hit areas. They hug the top corners of a tall card; on a one-row
/// card there is no "top corner" to speak of, so they centre against the row instead.
fn corner_rects(card: egui::Rect) -> (egui::Rect, egui::Rect) {
    let top = if card.height() < SHORT_CARD {
        card.center().y - CORNER_BUTTON / 2.0
    } else {
        card.top()
    };
    let size = Vec2::splat(CORNER_BUTTON);
    (
        egui::Rect::from_min_size(egui::pos2(card.left(), top), size),
        egui::Rect::from_min_size(egui::pos2(card.right() - CORNER_BUTTON, top), size),
    )
}

/// A quiet glyph in a card corner — a dash to minimise, a cross to close — that
/// lights up on hover so the resting card stays as plain as the rest of the face.
fn corner_button(ui: &mut egui::Ui, rect: egui::Rect, kind: CornerButton) -> egui::Response {
    let id = ui.id().with(match kind {
        CornerButton::Minimise => "minimise",
        CornerButton::Close => "close",
    });
    let response = ui.interact(rect, id, Sense::click());

    let color = match (kind, response.hovered()) {
        (_, false) => Color32::from_rgba_unmultiplied(255, 255, 255, 0x66),
        (CornerButton::Minimise, true) => Color32::WHITE,
        (CornerButton::Close, true) => styles::rgb(visuals::RED),
    };
    let stroke = Stroke::new(1.2_f32, color);
    let centre = rect.center();
    let arm = 3.0;

    let painter = ui.painter();
    match kind {
        CornerButton::Minimise => {
            painter.line_segment(
                [centre - Vec2::new(arm, 0.0), centre + Vec2::new(arm, 0.0)],
                stroke,
            );
        }
        CornerButton::Close => {
            painter.line_segment(
                [centre - Vec2::splat(arm), centre + Vec2::splat(arm)],
                stroke,
            );
            painter.line_segment(
                [centre + Vec2::new(-arm, arm), centre + Vec2::new(arm, -arm)],
                stroke,
            );
        }
    }

    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Whether a point in desktop pixels falls on a connected display.
///
/// Only Windows is asked. That is where a stranded widget was reported, and it is the
/// one platform whose query is reachable from here — eframe hands out no monitor
/// geometry, and egui reports only the size of the monitor a window is already on,
/// never where the screens sit. Elsewhere every position is accepted, which is what
/// the widget did everywhere before.
#[cfg(target_os = "windows")]
fn on_a_screen(point: egui::Pos2) -> bool {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::Graphics::Gdi::{MonitorFromPoint, MONITOR_DEFAULTTONULL};

    let point = POINT {
        x: point.x as i32,
        y: point.y as i32,
    };
    // Null is the documented answer for "that is not on any monitor".
    !unsafe { MonitorFromPoint(point, MONITOR_DEFAULTTONULL) }.is_null()
}

#[cfg(not(target_os = "windows"))]
fn on_a_screen(_point: egui::Pos2) -> bool {
    true
}

/// Where the window should be created. A widget that was hidden last time starts
/// parked off-screen, so it never flashes on screen before being moved.
pub fn initial_position(config: &AppConfig) -> Option<egui::Pos2> {
    if !config.widget_visible {
        return Some(PARKED);
    }
    match (config.widget_left, config.widget_top) {
        (Some(x), Some(y)) => Some(egui::pos2(x, y)),
        _ => None,
    }
}
