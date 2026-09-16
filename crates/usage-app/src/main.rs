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

    if let Some(position) = widget::initial_position(&config) {
        viewport = viewport.with_position(position);
    }

    let options = eframe::NativeOptions {
        viewport,
        // The card paints its own rounded background over a transparent window.
        centered: false,
        ..Default::default()
    };

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
