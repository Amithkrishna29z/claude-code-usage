//! System tray presence: two concentric colour-coded rings mirroring the widget,
//! plus a menu. The icon is rasterised by hand into an RGBA buffer so the app needs
//! no image decoder and no bundled asset files.

use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use usage_core::{UsageSnapshot, UsageState};

use crate::visuals;

const SIZE: u32 = 32;

/// What the user picked from the tray menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayCommand {
    ToggleWidget,
    Refresh,
    /// Re-read config.json from disk and apply it without restarting.
    ReloadSettings,
    Quit,
}

pub struct Tray {
    icon: TrayIcon,
    toggle_id: MenuId,
    refresh_id: MenuId,
    reload_id: MenuId,
    quit_id: MenuId,
    /// The rings last drawn, so we only rebuild the icon when it would change.
    last_drawn: Option<(i64, i64)>,
}

impl Tray {
    /// Must be called on the main thread — macOS requires it, and Windows needs the
    /// creating thread to own a message pump.
    pub fn new() -> Option<Self> {
        let toggle = MenuItem::new("Show/hide widget", true, None);
        let refresh = MenuItem::new("Refresh now", true, None);
        let reload = MenuItem::new("Reload settings", true, None);
        let quit = MenuItem::new("Quit", true, None);

        let menu = Menu::new();
        menu.append(&toggle).ok()?;
        menu.append(&refresh).ok()?;
        menu.append(&PredefinedMenuItem::separator()).ok()?;
        menu.append(&reload).ok()?;
        menu.append(&PredefinedMenuItem::separator()).ok()?;
        menu.append(&quit).ok()?;

        let placeholder = UsageSnapshot::empty(UsageState::NoData, 0, chrono::Utc::now());
        let icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("Claude Code Usage")
            .with_icon(render_icon(&placeholder))
            .build()
            .ok()?;

        Some(Self {
            icon,
            toggle_id: toggle.id().clone(),
            refresh_id: refresh.id().clone(),
            reload_id: reload.id().clone(),
            quit_id: quit.id().clone(),
            last_drawn: None,
        })
    }

    /// Drains pending tray interactions. Call once per frame.
    pub fn poll(&self) -> Option<TrayCommand> {
        // A left-click on the icon toggles the widget, matching the usual tray idiom.
        while let Ok(event) = TrayIconEvent::receiver().try_recv() {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                return Some(TrayCommand::ToggleWidget);
            }
        }

        while let Ok(event) = MenuEvent::receiver().try_recv() {
            if event.id == self.toggle_id {
                return Some(TrayCommand::ToggleWidget);
            }
            if event.id == self.refresh_id {
                return Some(TrayCommand::Refresh);
            }
            if event.id == self.reload_id {
                return Some(TrayCommand::ReloadSettings);
            }
            if event.id == self.quit_id {
                return Some(TrayCommand::Quit);
            }
        }
        None
    }

    pub fn update(&mut self, snapshot: &UsageSnapshot, now: chrono::DateTime<chrono::Utc>) {
        // Redraw only when a whole percent has moved — the icon is 32px, and
        // rebuilding it every frame is pure waste.
        let key = (
            (snapshot.session.map(|w| w.utilization).unwrap_or(-1.0) * 100.0).round() as i64,
            (snapshot.weekly.map(|w| w.utilization).unwrap_or(-1.0) * 100.0).round() as i64,
        );
        if self.last_drawn != Some(key) {
            self.last_drawn = Some(key);
            let _ = self.icon.set_icon(Some(render_icon(snapshot)));
        }

        let _ = self
            .icon
            .set_tooltip(Some(visuals::detail_text(snapshot, now)));
    }
}

/// Draws the two rings into a 32x32 RGBA buffer.
fn render_icon(snapshot: &UsageSnapshot) -> Icon {
    let mut pixels = vec![0u8; (SIZE * SIZE * 4) as usize];

    let live = snapshot.state == UsageState::Ok;
    let session_fraction = if live {
        snapshot.session.map(|w| w.utilization).unwrap_or(0.0)
    } else {
        0.0
    };

    // Outer ring: session.
    draw_ring(
        &mut pixels,
        13.0,
        2.0,
        session_fraction,
        visuals::session_color(snapshot),
    );

    // Inner ring: weekly, drawn only when the official source supplied it.
    if let Some(weekly) = snapshot.weekly {
        draw_ring(
            &mut pixels,
            8.0,
            1.5,
            weekly.utilization,
            visuals::color_for(Some(weekly)),
        );
    } else {
        // A centre dot conveys state at tiny sizes even when the arc is short.
        draw_disc(&mut pixels, 4.0, visuals::session_color(snapshot));
    }

    Icon::from_rgba(pixels, SIZE, SIZE).expect("32x32 RGBA buffer is a valid icon")
}

/// A faint full-circle track plus a clockwise progress arc starting at 12 o'clock.
/// Sampled 3x3 per pixel, which is enough to look smooth at 32px.
fn draw_ring(pixels: &mut [u8], radius: f32, half_width: f32, fraction: f64, color: [u8; 3]) {
    let sweep = fraction.clamp(0.0, 1.0) as f32 * std::f32::consts::TAU;
    let centre = SIZE as f32 / 2.0;
    const SAMPLES: u32 = 3;
    const TOTAL: f32 = (SAMPLES * SAMPLES) as f32;

    for y in 0..SIZE {
        for x in 0..SIZE {
            let mut in_band = 0u32;
            let mut in_arc = 0u32;

            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let px = x as f32 + (sx as f32 + 0.5) / SAMPLES as f32;
                    let py = y as f32 + (sy as f32 + 0.5) / SAMPLES as f32;
                    let dx = px - centre;
                    let dy = py - centre;

                    if ((dx * dx + dy * dy).sqrt() - radius).abs() > half_width {
                        continue;
                    }
                    in_band += 1;

                    // Angle measured clockwise from 12 o'clock.
                    let angle = dx.atan2(-dy).rem_euclid(std::f32::consts::TAU);
                    if sweep > 0.0 && angle <= sweep {
                        in_arc += 1;
                    }
                }
            }

            if in_band == 0 {
                continue;
            }

            // The unfilled remainder of the band is the track; the rest is the arc.
            let track = (in_band - in_arc) as f32 / TOTAL;
            if track > 0.0 {
                blend(pixels, x, y, [90, 90, 90], track * 0.45);
            }
            if in_arc > 0 {
                blend(pixels, x, y, color, in_arc as f32 / TOTAL);
            }
        }
    }
}

fn draw_disc(pixels: &mut [u8], radius: f32, color: [u8; 3]) {
    let centre = SIZE as f32 / 2.0;
    for y in 0..SIZE {
        for x in 0..SIZE {
            let dx = x as f32 + 0.5 - centre;
            let dy = y as f32 + 0.5 - centre;
            if (dx * dx + dy * dy).sqrt() <= radius {
                blend(pixels, x, y, color, 1.0);
            }
        }
    }
}

/// Source-over composite onto a transparent buffer.
fn blend(pixels: &mut [u8], x: u32, y: u32, color: [u8; 3], alpha: f32) {
    let alpha = alpha.clamp(0.0, 1.0);
    if alpha <= 0.0 {
        return;
    }
    let idx = ((y * SIZE + x) * 4) as usize;
    let existing = pixels[idx + 3] as f32 / 255.0;
    let out_alpha = alpha + existing * (1.0 - alpha);
    for c in 0..3 {
        let src = color[c] as f32 / 255.0;
        let dst = pixels[idx + c] as f32 / 255.0;
        let value = (src * alpha + dst * existing * (1.0 - alpha)) / out_alpha.max(1e-6);
        pixels[idx + c] = (value.clamp(0.0, 1.0) * 255.0) as u8;
    }
    pixels[idx + 3] = (out_alpha * 255.0) as u8;
}
