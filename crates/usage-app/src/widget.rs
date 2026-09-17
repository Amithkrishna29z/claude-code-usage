//! The always-on-top mini widget: two concentric rings, the session percent, and one
//! compact line. Everything else (both reset times, the data source, freshness)
//! lives in the hover tooltip, so the resting state stays quiet without losing
//! information.

use chrono::{Duration, Utc};
use eframe::egui::{self, Color32, FontId, Pos2, Sense, Stroke, Vec2, ViewportCommand};
use usage_core::{AppConfig, ConfigService, UsageSnapshot, UsageState};

use crate::monitor::UsageMonitor;
use crate::tray::{Tray, TrayCommand};
use crate::visuals;

pub const WIDGET_SIZE: Vec2 = Vec2::new(122.0, 142.0);

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

/// How long without new Claude activity before the widget shows "stale".
const STALE_AFTER_MINUTES: i64 = 10;

/// Ring geometry, as fractions of the ring box.
const OUTER_RADIUS: f32 = 46.0;
const OUTER_WIDTH: f32 = 8.0;
const INNER_RADIUS: f32 = 34.0;
const INNER_WIDTH: f32 = 5.5;

pub struct WidgetApp {
    config: AppConfig,
    config_service: ConfigService,
    monitor: UsageMonitor,
    snapshot: UsageSnapshot,
    /// Created lazily on the first frame: the tray must be built on the main thread.
    tray: Option<Tray>,
    tray_attempted: bool,
    visible: bool,
}

impl WidgetApp {
    pub fn new(config: AppConfig, config_service: ConfigService, monitor: UsageMonitor) -> Self {
        let visible = config.widget_visible;
        Self {
            snapshot: UsageSnapshot::empty(UsageState::NoData, config.token_limit, Utc::now()),
            config,
            config_service,
            monitor,
            tray: None,
            tray_attempted: false,
            visible,
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
            ctx.send_viewport_cmd(ViewportCommand::OuterPosition(self.shown_position()));
            ctx.send_viewport_cmd(ViewportCommand::Focus);
        } else {
            ctx.send_viewport_cmd(ViewportCommand::OuterPosition(PARKED));
        }

        self.persist();
    }

    fn shown_position(&self) -> egui::Pos2 {
        match (self.config.widget_left, self.config.widget_top) {
            (Some(x), Some(y)) => egui::pos2(x, y),
            _ => DEFAULT_POSITION,
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
            self.tray = Tray::new();
            if self.tray.is_none() {
                eprintln!("usage: no system tray available; widget runs standalone.");
            }
            if !self.visible {
                ctx.send_viewport_cmd(ViewportCommand::OuterPosition(PARKED));
            }
        }

        if let Some(latest) = self.monitor.latest() {
            self.snapshot = latest;
        }

        match self.tray.as_ref().and_then(Tray::poll) {
            Some(TrayCommand::ToggleWidget) => self.toggle_widget(ctx),
            Some(TrayCommand::Refresh) => self.monitor.refresh_now(),
            Some(TrayCommand::ReloadSettings) => self.reload_settings(),
            Some(TrayCommand::Quit) => {
                self.persist();
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
            None => {}
        }

        let now = Utc::now();
        if let Some(tray) = self.tray.as_mut() {
            tray.update(&self.snapshot, now);
        }

        if self.visible {
            self.draw_card(ctx, now);
        }

        // Keep relative times live, and keep polling the tray even while the window
        // is hidden — without this the menu would stop responding once hidden.
        ctx.request_repaint_after(std::time::Duration::from_millis(200));
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

        egui::CentralPanel::default().frame(frame).show(ctx, |ui| {
            let response =
                ui.interact(ui.max_rect(), ui.id().with("drag"), Sense::click_and_drag());
            if response.drag_started() {
                ctx.send_viewport_cmd(ViewportCommand::StartDrag);
            }

            ui.vertical_centered(|ui| {
                self.draw_rings(ui, now);
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(self.compact_line(now))
                        .size(9.0)
                        .color(Color32::from_rgba_unmultiplied(255, 255, 255, 0xAA)),
                );
            });
        });

        self.remember_position(ctx);
    }

    fn draw_rings(&self, ui: &mut egui::Ui, now: chrono::DateTime<Utc>) {
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(102.0), Sense::hover());
        let painter = ui.painter();
        let centre = rect.center();

        let live = matches!(
            self.snapshot.state,
            UsageState::Ok | UsageState::NoActiveSession
        );

        let session_color = if live {
            rgb(visuals::color_for(self.snapshot.session))
        } else {
            rgb(visuals::GREY)
        };

        // Outer ring: session.
        arc(
            painter,
            centre,
            OUTER_RADIUS,
            OUTER_WIDTH,
            1.0,
            track_color(),
        );
        if live {
            let fraction = self.snapshot.session.map(|w| w.utilization).unwrap_or(0.0);
            arc(
                painter,
                centre,
                OUTER_RADIUS,
                OUTER_WIDTH,
                fraction,
                session_color,
            );
        }

        // Inner ring: weekly.
        arc(
            painter,
            centre,
            INNER_RADIUS,
            INNER_WIDTH,
            1.0,
            track_color(),
        );
        if let Some(weekly) = self.snapshot.weekly {
            arc(
                painter,
                centre,
                INNER_RADIUS,
                INNER_WIDTH,
                weekly.utilization,
                rgb(visuals::color_for(Some(weekly))),
            );
        }

        // Centre: the session number, because that is the one that bites first.
        let percent = if live {
            visuals::format_percent(self.snapshot.session)
        } else {
            "—".to_owned()
        };
        painter.text(
            centre - Vec2::new(0.0, 5.0),
            egui::Align2::CENTER_CENTER,
            percent,
            FontId::proportional(22.0),
            Color32::WHITE,
        );

        let caption = self.caption(now);
        if !caption.is_empty() {
            painter.text(
                centre + Vec2::new(0.0, 13.0),
                egui::Align2::CENTER_CENTER,
                caption,
                FontId::proportional(8.0),
                Color32::from_rgba_unmultiplied(255, 255, 255, 0x99),
            );
        }
    }

    /// The caption under the percent. Blank when everything is healthy and official —
    /// a minimal face should say nothing when there is nothing to flag.
    fn caption(&self, now: chrono::DateTime<Utc>) -> String {
        if self.snapshot.state == UsageState::Ok {
            if let Some(last) = self.snapshot.last_activity {
                if now - last > Duration::minutes(STALE_AFTER_MINUTES) {
                    return "stale".to_owned();
                }
            }
        }
        visuals::state_caption(&self.snapshot).to_owned()
    }

    /// The one visible line under the ring: the weekly figure the centre cannot show,
    /// and how long the session window has left. Either half is dropped when unknown
    /// rather than rendered as a dash.
    fn compact_line(&self, now: chrono::DateTime<Utc>) -> String {
        let mut parts: Vec<String> = Vec::with_capacity(2);

        if let Some(weekly) = self.snapshot.weekly {
            parts.push(format!("wk {}", visuals::format_percent(Some(weekly))));
        }
        if self.snapshot.reset_at().is_some() {
            parts.push(format!(
                "{} left",
                visuals::format_duration(self.snapshot.time_until_reset(now))
            ));
        }

        parts.join("  ·  ")
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
        if left <= PARKED.x + 1.0 || top <= PARKED.y + 1.0 {
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

fn rgb(color: [u8; 3]) -> Color32 {
    Color32::from_rgb(color[0], color[1], color[2])
}

fn track_color() -> Color32 {
    Color32::from_rgba_unmultiplied(255, 255, 255, 0x26)
}

/// Draws a clockwise arc from 12 o'clock as a run of short line segments with round
/// joins, which is how egui's painter handles stroked curves.
fn arc(
    painter: &egui::Painter,
    centre: Pos2,
    radius: f32,
    width: f32,
    fraction: f64,
    color: Color32,
) {
    let fraction = fraction.clamp(0.0, 1.0) as f32;
    if fraction <= 0.0 {
        return;
    }

    let sweep = fraction * std::f32::consts::TAU;
    // Enough segments that the curve reads as smooth at this radius.
    let steps = ((sweep * radius / 2.0).ceil() as usize).clamp(8, 180);

    let points: Vec<Pos2> = (0..=steps)
        .map(|i| {
            let angle = sweep * (i as f32 / steps as f32);
            centre + Vec2::new(angle.sin() * radius, -angle.cos() * radius)
        })
        .collect();

    painter.add(egui::Shape::line(points.clone(), Stroke::new(width, color)));

    // egui strokes polylines with butt ends; discs at the tips give the round caps
    // the design calls for. Skipped on a full circle, where the ends meet anyway.
    if fraction < 1.0 {
        let cap = width / 2.0;
        if let (Some(first), Some(last)) = (points.first(), points.last()) {
            painter.circle_filled(*first, cap, color);
            painter.circle_filled(*last, cap, color);
        }
    }
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
