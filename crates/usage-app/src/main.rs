//! Claude Code Usage — a cross-platform tray app + always-on-top mini widget showing
//! how much of your Claude Code allowance is gone.
//!
//! No console window on Windows release builds; the app lives in the tray.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod monitor;
mod tray;
mod visuals;
mod widget;

use eframe::egui;
use usage_core::ConfigService;

use crate::monitor::UsageMonitor;
use crate::widget::{WidgetApp, WIDGET_SIZE};

fn main() -> eframe::Result<()> {
    let config_service = ConfigService::new();
    let config = config_service.load();

    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size(WIDGET_SIZE)
        .with_min_inner_size(WIDGET_SIZE)
        .with_max_inner_size(WIDGET_SIZE)
        .with_resizable(false)
        .with_decorations(false)
        .with_transparent(true)
        .with_always_on_top()
        .with_taskbar(false)
        .with_title("Claude Code Usage");

    // X11 has no equivalent of `with_taskbar(false)`: the hint that keeps a small
    // floating panel out of the taskbar and above ordinary windows is its window
    // type. Without it the widget is just another top-level window and sinks behind
    // whatever you switch to.
    #[cfg(target_os = "linux")]
    {
        viewport = viewport.with_window_type(egui::X11WindowType::Utility);
    }

    if let Some(position) = widget::initial_position(&config) {
        viewport = viewport.with_position(position);
    }

    #[allow(unused_mut)]
    let mut options = eframe::NativeOptions {
        viewport,
        // The card paints its own rounded background over a transparent window.
        centered: false,
        ..Default::default()
    };

    #[cfg(target_os = "linux")]
    prefer_x11(&mut options);

    eframe::run_native(
        "Claude Code Usage",
        options,
        Box::new(move |cc| {
            let ctx = cc.egui_ctx.clone();
            // The worker wakes the UI thread the moment a snapshot lands, so the
            // display never waits on the next repaint tick.
            let monitor = UsageMonitor::start(config.clone(), move || ctx.request_repaint());
            Ok(Box::new(WidgetApp::new(config, config_service, monitor)))
        }),
    )
}

/// Wayland offers no always-on-top: winit's `set_window_level` is an empty function
/// there, and `set_outer_position` is ignored too, so the widget sinks behind the app
/// you switch to and cannot be parked or restored. XWayland speaks X11, where both
/// work, so run through it whenever a Wayland session also has an X server.
/// `CLAUDE_USAGE_BACKEND=wayland` opts back out.
#[cfg(target_os = "linux")]
fn prefer_x11(options: &mut eframe::NativeOptions) {
    use winit::platform::x11::EventLoopBuilderExtX11 as _;

    let is_set = |name: &str| std::env::var(name).is_ok_and(|value| !value.is_empty());

    let wayland = is_set("WAYLAND_DISPLAY") || is_set("WAYLAND_SOCKET");
    let opted_out = std::env::var("CLAUDE_USAGE_BACKEND")
        .is_ok_and(|value| value.eq_ignore_ascii_case("wayland"));

    if wayland && is_set("DISPLAY") && !opted_out {
        options.event_loop_builder = Some(Box::new(|builder| {
            builder.with_x11();
        }));
    }
}
