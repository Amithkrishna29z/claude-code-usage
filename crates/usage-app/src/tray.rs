//! System tray presence: two concentric colour-coded rings mirroring the widget,
//! plus a menu. The icon is rasterised by hand into an RGBA buffer so the app needs
//! no image decoder and no bundled asset files.

use std::cell::RefCell;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::OnceLock;

use tray_icon::menu::{
    CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu,
};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use usage_core::{UsageSnapshot, UsageState, WidgetStyle};

use crate::visuals;

const SIZE: u32 = 32;

/// What the user picked from the tray menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayCommand {
    ToggleWidget,
    Refresh,
    /// Re-read config.json from disk and apply it without restarting.
    ReloadSettings,
    /// Wear a different face, picked from the Style submenu.
    SetStyle(WidgetStyle),
    Quit,
}

/// One raw interaction, forwarded from tray-icon's global handlers.
enum TrayEvent {
    Click,
    Menu(MenuId),
}

/// tray-icon delivers events through process-wide handlers, and installing one
/// replaces its channel — so this is done exactly once and everything is funnelled
/// into a channel of our own.
static EVENTS: OnceLock<Sender<TrayEvent>> = OnceLock::new();

/// Routes tray and menu events into `sender` and wakes the UI thread for each one.
///
/// The wake is the point: without it a click is only noticed when the next frame
/// happens to run, which is a fifth of a second of nothing after every menu pick. With
/// it the UI can idle as slowly as it likes and still react the instant you click.
fn install_handlers(sender: Sender<TrayEvent>, wake: impl Fn() + Send + Sync + 'static) {
    if EVENTS.set(sender).is_err() {
        return; // already installed; handlers are global and set-once
    }
    let wake = std::sync::Arc::new(wake);

    let menu_wake = wake.clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        // The click has already flipped the item muda owns, so the group is wrong
        // right now. Put it right here, inside the menu's own modal loop, rather than
        // leaving it to the frame that cannot run until the menu closes. A pick that
        // is not a face, or a handler called off the menu's thread, finds nothing and
        // leaves this to `Tray::set_style` as before.
        if let Some(style) = style_for(&event.id) {
            retick(style);
        }
        if let Some(tx) = EVENTS.get() {
            let _ = tx.send(TrayEvent::Menu(event.id));
        }
        menu_wake();
    }));

    TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
        if let TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        } = event
        {
            if let Some(tx) = EVENTS.get() {
                let _ = tx.send(TrayEvent::Click);
            }
            wake();
        }
    }));
}

/// The menu item ids, kept on the main thread so `poll` can map an event back to a
/// command. On Linux the items are built on the GTK thread, so the ids travel back.
struct MenuIds {
    toggle: MenuId,
    refresh: MenuId,
    reload: MenuId,
    quit: MenuId,
    /// One id per style, so a click in the submenu maps straight back to a face.
    styles: Vec<(WidgetStyle, MenuId)>,
}

/// The submenu's check items. They are not `Send`, so they stay on whichever thread
/// built the menu — the main thread everywhere but Linux, the GTK thread there.
type StyleItems = Vec<(WidgetStyle, CheckMenuItem)>;

thread_local! {
    /// The Style submenu's check items, parked on the thread that built them.
    ///
    /// This is what lets the menu event handler re-tick the group the instant a pick
    /// arrives. Correcting the ticks from `update()` is too late on Windows: the menu
    /// runs a nested modal loop on the main thread, so no frame can run until the
    /// menu closes, and until then the submenu shows muda's own half-applied state —
    /// two faces ticked after a change, none at all after re-picking the current one.
    /// muda calls the handler on this same thread, so the items are reachable there
    /// and nowhere else.
    static STYLE_ITEMS: RefCell<StyleItems> = const { RefCell::new(Vec::new()) };
}

/// Hands the check items to the thread that will re-tick them. Call on the thread that
/// built the menu.
fn park_style_items(items: StyleItems) {
    STYLE_ITEMS.with(|cell| *cell.borrow_mut() = items);
}

/// Leaves exactly one item ticked, from whichever thread owns them.
fn retick(style: WidgetStyle) {
    STYLE_ITEMS.with(|cell| check_only(&cell.borrow(), style));
}

/// The face `id` belongs to, if it is one of the Style submenu's items.
fn style_for(id: &MenuId) -> Option<WidgetStyle> {
    STYLE_ITEMS.with(|cell| {
        cell.borrow()
            .iter()
            .find(|(_, item)| item.id() == id)
            .map(|(style, _)| *style)
    })
}

/// What the main thread asks the Linux GTK thread to apply. `TrayIcon` is not `Send`,
/// so the pixels cross the channel and the `Icon` is built on the far side.
#[cfg(target_os = "linux")]
enum TrayUpdate {
    Icon(Vec<u8>),
    Tooltip(String),
    Style(WidgetStyle),
}

pub struct Tray {
    #[cfg(not(target_os = "linux"))]
    icon: TrayIcon,
    #[cfg(target_os = "linux")]
    updates: std::sync::mpsc::Sender<TrayUpdate>,
    ids: MenuIds,
    /// Raw interactions, delivered by the global handlers.
    events: Receiver<TrayEvent>,
    /// The rings last drawn, so we only rebuild the icon when it would change.
    last_drawn: Option<(i64, i64)>,
    /// The tooltip last pushed to the shell. Setting a tray tooltip is a syscall, and
    /// re-sending an unchanged string every frame is enough shell traffic to make the
    /// app look like it has stopped responding.
    last_tooltip: Option<String>,
}

impl Tray {
    /// Must be called on the main thread — macOS requires it, and Windows needs the
    /// creating thread to own a message pump.
    #[cfg(not(target_os = "linux"))]
    pub fn new(style: WidgetStyle, wake: impl Fn() + Send + Sync + 'static) -> Option<Self> {
        let (menu, ids, style_items) = build_menu(style)?;
        let icon = build_tray(menu).ok()?;

        // This is the main thread, which is where muda will deliver menu events.
        park_style_items(style_items);

        let (sender, events) = mpsc::channel();
        install_handlers(sender, wake);

        Some(Self {
            icon,
            ids,
            events,
            last_drawn: None,
            last_tooltip: None,
        })
    }

    /// Linux wants the tray on a thread running a GTK main loop, which eframe's winit
    /// loop is not: `gtk::init` has never been called there, so building a menu on it
    /// panics outright. So GTK gets a thread of its own, owns the tray icon for the
    /// life of the process, and takes updates over a channel. Clicks and menu
    /// activations still arrive through tray-icon's global receivers, which are
    /// cross-thread, so `poll` stays on the main thread unchanged.
    #[cfg(target_os = "linux")]
    pub fn new(style: WidgetStyle, wake: impl Fn() + Send + Sync + 'static) -> Option<Self> {
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Option<MenuIds>>();
        let (updates, incoming) = std::sync::mpsc::channel::<TrayUpdate>();

        std::thread::Builder::new()
            .name("tray-gtk".to_owned())
            .spawn(move || {
                if gtk::init().is_err() {
                    let _ = ready_tx.send(None);
                    return;
                }

                let tray = build_menu(style).and_then(|(menu, ids, style_items)| {
                    let tray = build_tray(menu).ok()?;
                    Some((tray, ids, style_items))
                });
                let Some((tray, ids, style_items)) = tray else {
                    let _ = ready_tx.send(None);
                    return;
                };
                // The GTK thread is where muda delivers menu events on Linux.
                park_style_items(style_items);
                // Drained from inside the GTK loop, since the icon cannot leave this
                // thread. Half a second is imperceptible for a 5-hour window. Armed
                // before the handshake, so that if it fails the main thread hears
                // about it through the dropped sender rather than assuming a tray.
                gtk::glib::timeout_add_local(std::time::Duration::from_millis(500), move || {
                    while let Ok(update) = incoming.try_recv() {
                        match update {
                            TrayUpdate::Icon(pixels) => {
                                let _ = tray.set_icon(Some(icon_from(pixels)));
                            }
                            TrayUpdate::Tooltip(text) => {
                                let _ = tray.set_tooltip(Some(text));
                            }
                            // The check items live on this thread, so the main
                            // thread asks for the tick rather than moving it.
                            TrayUpdate::Style(style) => retick(style),
                        }
                    }
                    gtk::glib::ControlFlow::Continue
                });

                if ready_tx.send(Some(ids)).is_err() {
                    return;
                }

                gtk::main();
            })
            .ok()?;

        // Blocks only until GTK is up, and returns None if that thread gave up.
        let ids = ready_rx.recv().ok().flatten()?;

        let (sender, events) = mpsc::channel();
        install_handlers(sender, wake);

        Some(Self {
            updates,
            ids,
            events,
            last_drawn: None,
            last_tooltip: None,
        })
    }

    /// Takes the next pending interaction, if any. Call until it returns `None`, so a
    /// burst of clicks is handled in the frame it arrives rather than one per frame.
    pub fn poll(&self) -> Option<TrayCommand> {
        while let Ok(event) = self.events.try_recv() {
            // A left-click on the icon toggles the widget, the usual tray idiom.
            let id = match event {
                TrayEvent::Click => return Some(TrayCommand::ToggleWidget),
                TrayEvent::Menu(id) => id,
            };

            if id == self.ids.toggle {
                return Some(TrayCommand::ToggleWidget);
            }
            if id == self.ids.refresh {
                return Some(TrayCommand::Refresh);
            }
            if id == self.ids.reload {
                return Some(TrayCommand::ReloadSettings);
            }
            if id == self.ids.quit {
                return Some(TrayCommand::Quit);
            }
            // A check item toggles itself on click, so whatever the user hit, the
            // widget re-ticks the whole group from the style it actually applied.
            if let Some((style, _)) = self.ids.styles.iter().find(|(_, sid)| *sid == id) {
                return Some(TrayCommand::SetStyle(*style));
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
            self.set_icon(render_pixels(snapshot));
        }

        let tooltip = visuals::detail_text(snapshot, now);
        if self.last_tooltip.as_deref() != Some(tooltip.as_str()) {
            self.last_tooltip = Some(tooltip.clone());
            self.set_tooltip(tooltip);
        }
    }

    /// Ticks exactly one item in the Style submenu.
    ///
    /// The handler has usually done this already, the moment the pick arrived. This
    /// remains for the ticks that no click produced: the style reloaded from a
    /// hand-edited config.json, and the one applied at startup.
    #[cfg(not(target_os = "linux"))]
    pub fn set_style(&self, style: WidgetStyle) {
        retick(style);
    }

    #[cfg(target_os = "linux")]
    pub fn set_style(&self, style: WidgetStyle) {
        let _ = self.updates.send(TrayUpdate::Style(style));
    }

    #[cfg(not(target_os = "linux"))]
    fn set_icon(&self, pixels: Vec<u8>) {
        let _ = self.icon.set_icon(Some(icon_from(pixels)));
    }

    #[cfg(not(target_os = "linux"))]
    fn set_tooltip(&self, text: String) {
        let _ = self.icon.set_tooltip(Some(text));
    }

    #[cfg(target_os = "linux")]
    fn set_icon(&self, pixels: Vec<u8>) {
        let _ = self.updates.send(TrayUpdate::Icon(pixels));
    }

    /// Ayatana ignores tooltips, so on Linux the detail text is sent and dropped;
    /// tray-icon keeps the call as a no-op rather than an error.
    #[cfg(target_os = "linux")]
    fn set_tooltip(&self, text: String) {
        let _ = self.updates.send(TrayUpdate::Tooltip(text));
    }
}

/// The menu is identical on every platform; only the thread it is built on differs.
/// `style` is the face to show ticked when the menu first opens.
fn build_menu(style: WidgetStyle) -> Option<(Menu, MenuIds, StyleItems)> {
    let toggle = MenuItem::new("Show/hide widget", true, None);
    let refresh = MenuItem::new("Refresh now", true, None);
    let reload = MenuItem::new("Reload settings", true, None);
    let quit = MenuItem::new("Quit", true, None);

    let styles_menu = Submenu::new("Style", true);
    let style_items: StyleItems = WidgetStyle::ALL
        .into_iter()
        .map(|s| (s, CheckMenuItem::new(s.label(), true, s == style, None)))
        .collect();
    for (_, item) in &style_items {
        styles_menu.append(item).ok()?;
    }

    let menu = Menu::new();
    menu.append(&toggle).ok()?;
    menu.append(&refresh).ok()?;
    menu.append(&PredefinedMenuItem::separator()).ok()?;
    menu.append(&styles_menu).ok()?;
    menu.append(&reload).ok()?;
    menu.append(&PredefinedMenuItem::separator()).ok()?;
    menu.append(&quit).ok()?;

    let ids = MenuIds {
        toggle: toggle.id().clone(),
        refresh: refresh.id().clone(),
        reload: reload.id().clone(),
        quit: quit.id().clone(),
        styles: style_items
            .iter()
            .map(|(s, item)| (*s, item.id().clone()))
            .collect(),
    };

    Some((menu, ids, style_items))
}

/// Leaves exactly one item checked. Called after every pick, including a re-pick of
/// the face already in use, which a check item would otherwise untick itself.
fn check_only(items: &StyleItems, style: WidgetStyle) {
    for (candidate, item) in items {
        item.set_checked(*candidate == style);
    }
}

fn build_tray(menu: Menu) -> tray_icon::Result<TrayIcon> {
    let placeholder = UsageSnapshot::empty(UsageState::NoData, 0, chrono::Utc::now());
    TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("Claude Code Usage")
        .with_icon(icon_from(render_pixels(&placeholder)))
        .build()
}

fn icon_from(pixels: Vec<u8>) -> Icon {
    Icon::from_rgba(pixels, SIZE, SIZE).expect("32x32 RGBA buffer is a valid icon")
}

/// Draws the two rings into a 32x32 RGBA buffer.
fn render_pixels(snapshot: &UsageSnapshot) -> Vec<u8> {
    let mut pixels = vec![0u8; (SIZE * SIZE * 4) as usize];

    let live = snapshot.state == UsageState::Ok;
    let session_fraction = if live {
        snapshot.session.map(|w| w.utilization).unwrap_or(0.0)
    } else {
        0.0
    };

    // Outer ring: weekly, drawn only when the official source supplied it. The
    // fallback has no weekly figure, so only the faint track shows there.
    if let Some(weekly) = snapshot.weekly {
        draw_ring(
            &mut pixels,
            13.0,
            2.0,
            weekly.utilization,
            visuals::color_for(Some(weekly)),
        );
    }

    // Inner ring: session. Drawn second so it wins any overlap at this size.
    draw_ring(
        &mut pixels,
        8.0,
        1.5,
        session_fraction,
        visuals::session_color(snapshot),
    );

    // A centre dot keeps the session colour legible at 32px even when its arc is
    // short — the inner ring alone is only a few pixels of colour near 0%.
    draw_disc(&mut pixels, 3.0, visuals::session_color(snapshot));

    pixels
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
