//! The widget's interchangeable faces, picked from the tray's *Style* submenu.
//!
//! Each face declares the window size it wants and paints into the card that
//! [`crate::widget`] owns. Two rules bind all of them: keep the top corners clear,
//! because the minimise and close glyphs sit there on every style, and say nothing
//! when there is nothing to flag — the detail lives in the tray tooltip.

use chrono::{DateTime, Duration, Utc};
use eframe::egui::{self, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};
use usage_core::{UsageSnapshot, UsageState, WidgetStyle};

use crate::visuals;

/// How long without new Claude activity before a face shows "stale".
const STALE_AFTER_MINUTES: i64 = 10;

/// Ring geometry for [`WidgetStyle::Rings`]. Outer is the 7-day weekly window, inner
/// the 5-hour session.
const OUTER_RADIUS: f32 = 46.0;
const OUTER_WIDTH: f32 = 8.0;
const INNER_RADIUS: f32 = 34.0;
const INNER_WIDTH: f32 = 5.5;

/// Width kept free at each end of the one-row faces, so the text never runs under a
/// corner glyph. A little wider than the button in [`crate::widget`], so the two do
/// not merely miss each other but read as separate.
const GUTTER: f32 = 18.0;

/// The window size each face asks for, in logical points. Changing a face resizes the
/// window to match, so these are the whole of the layout contract between a style and
/// the card around it.
pub fn size_of(style: WidgetStyle) -> Vec2 {
    match style {
        WidgetStyle::Rings => Vec2::new(122.0, 142.0),
        WidgetStyle::Bars => Vec2::new(172.0, 88.0),
        WidgetStyle::Pill => Vec2::new(196.0, 40.0),
        WidgetStyle::Minimal => Vec2::new(72.0, 78.0),
        WidgetStyle::Text => Vec2::new(244.0, 32.0),
    }
}

/// Paints the chosen face into the card's content area.
pub fn draw(style: WidgetStyle, ui: &mut egui::Ui, snapshot: &UsageSnapshot, now: DateTime<Utc>) {
    match style {
        WidgetStyle::Rings => rings(ui, snapshot, now),
        WidgetStyle::Bars => bars(ui, snapshot, now),
        WidgetStyle::Pill => pill(ui, snapshot, now),
        WidgetStyle::Minimal => minimal(ui, snapshot, now),
        WidgetStyle::Text => text(ui, snapshot, now),
    }
}

/// Two concentric rings, the session percent, and one compact line — the original
/// face. The rings are inset far enough that the card's corners stay clear.
fn rings(ui: &mut egui::Ui, snapshot: &UsageSnapshot, now: DateTime<Utc>) {
    ui.vertical_centered(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(102.0), Sense::hover());
        let painter = ui.painter();
        let centre = rect.center();
        let live = is_live(snapshot);

        // Outer ring: the 7-day weekly window.
        arc(painter, centre, OUTER_RADIUS, OUTER_WIDTH, 1.0, track());
        if let Some(weekly) = snapshot.weekly {
            arc(
                painter,
                centre,
                OUTER_RADIUS,
                OUTER_WIDTH,
                weekly.utilization,
                rgb(visuals::color_for(Some(weekly))),
            );
        }

        // Inner ring: the 5-hour session window, sitting next to the percent in the
        // centre that reports the same number.
        arc(painter, centre, INNER_RADIUS, INNER_WIDTH, 1.0, track());
        if live {
            arc(
                painter,
                centre,
                INNER_RADIUS,
                INNER_WIDTH,
                snapshot.percent_used(),
                rgb(visuals::session_color(snapshot)),
            );
        }

        // Centre: the session number, because that is the one that bites first.
        painter.text(
            centre - Vec2::new(0.0, 5.0),
            egui::Align2::CENTER_CENTER,
            session_percent(snapshot),
            FontId::proportional(22.0),
            Color32::WHITE,
        );

        let caption = caption(snapshot, now);
        if !caption.is_empty() {
            painter.text(
                centre + Vec2::new(0.0, 13.0),
                egui::Align2::CENTER_CENTER,
                caption,
                FontId::proportional(8.0),
                dim(0x99),
            );
        }

        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(weekly_and_time(snapshot, now))
                .size(9.0)
                .color(dim(0xAA)),
        );
    });
}

/// Two labelled horizontal bars with a footer. The first bar starts below the corner
/// glyphs, since a full-width bar would otherwise run under them.
fn bars(ui: &mut egui::Ui, snapshot: &UsageSnapshot, now: DateTime<Utc>) {
    let rect = ui.max_rect();
    let painter = ui.painter();
    let live = is_live(snapshot);

    bar_row(
        painter,
        rect,
        rect.top() + 22.0,
        "session",
        if live { snapshot.session } else { None },
        rgb(visuals::session_color(snapshot)),
    );
    bar_row(
        painter,
        rect,
        rect.top() + 42.0,
        "weekly",
        snapshot.weekly,
        rgb(visuals::color_for(snapshot.weekly)),
    );

    let footer = footer_line(snapshot, now);
    if !footer.is_empty() {
        painter.text(
            Pos2::new(rect.center().x, rect.bottom() - 8.0),
            egui::Align2::CENTER_CENTER,
            footer,
            FontId::proportional(9.0),
            dim(0xAA),
        );
    }
}

/// One row of the bars face: name on the left, track and fill in the middle, percent
/// right-aligned.
fn bar_row(
    painter: &egui::Painter,
    rect: Rect,
    y: f32,
    label: &str,
    window: Option<usage_core::UsageWindow>,
    color: Color32,
) {
    painter.text(
        Pos2::new(rect.left(), y),
        egui::Align2::LEFT_CENTER,
        label,
        FontId::proportional(8.5),
        dim(0x99),
    );

    let left = rect.left() + 38.0;
    let right = rect.right() - 30.0;
    let track_rect = Rect::from_min_max(Pos2::new(left, y - 3.0), Pos2::new(right, y + 3.0));
    painter.rect_filled(track_rect, 3.0, track());

    if let Some(window) = window {
        let fraction = window.utilization.clamp(0.0, 1.0) as f32;
        if fraction > 0.0 {
            let filled = Rect::from_min_max(
                track_rect.min,
                Pos2::new(left + (right - left) * fraction, track_rect.max.y),
            );
            painter.rect_filled(filled, 3.0, color);
        }
    }

    painter.text(
        Pos2::new(rect.right(), y),
        egui::Align2::RIGHT_CENTER,
        visuals::format_percent(window),
        FontId::proportional(9.5),
        if window.is_some() { color } else { dim(0x99) },
    );
}

/// A single short row: a small dial, the session percent, and the time left. The ends
/// are left free for the corner glyphs, which centre themselves on a card this short.
fn pill(ui: &mut egui::Ui, snapshot: &UsageSnapshot, now: DateTime<Utc>) {
    let rect = ui.max_rect();
    let painter = ui.painter();
    let y = rect.center().y;
    let left = rect.left() + GUTTER;
    let color = rgb(visuals::session_color(snapshot));

    let dial = Pos2::new(left + 9.0, y);
    arc(painter, dial, 8.0, 3.0, 1.0, track());
    if is_live(snapshot) {
        arc(painter, dial, 8.0, 3.0, snapshot.percent_used(), color);
    }

    painter.text(
        Pos2::new(left + 24.0, y),
        egui::Align2::LEFT_CENTER,
        session_percent(snapshot),
        FontId::proportional(13.0),
        Color32::WHITE,
    );

    let trailing = footer_line(snapshot, now);
    if !trailing.is_empty() {
        painter.text(
            Pos2::new(rect.right() - GUTTER, y),
            egui::Align2::RIGHT_CENTER,
            trailing,
            FontId::proportional(9.0),
            dim(0xAA),
        );
    }
}

/// The session percent alone, in the traffic-light colour, with a caption only when
/// something needs flagging. The smallest face there is.
fn minimal(ui: &mut egui::Ui, snapshot: &UsageSnapshot, now: DateTime<Utc>) {
    let rect = ui.max_rect();
    let painter = ui.painter();
    let centre = rect.center();

    painter.text(
        centre,
        egui::Align2::CENTER_CENTER,
        session_percent(snapshot),
        FontId::proportional(26.0),
        rgb(visuals::session_color(snapshot)),
    );

    let caption = caption(snapshot, now);
    if !caption.is_empty() {
        painter.text(
            Pos2::new(centre.x, rect.bottom() - 6.0),
            egui::Align2::CENTER_CENTER,
            caption,
            FontId::proportional(8.0),
            dim(0x99),
        );
    }
}

/// One line of text, no graphics: session, weekly, time left. Only the session
/// percent is coloured, so the line still carries the traffic light. The two runs are
/// measured and centred as a group, since the weekly figure is not always there and a
/// line pinned to one end would sit lopsided whenever it is missing.
fn text(ui: &mut egui::Ui, snapshot: &UsageSnapshot, now: DateTime<Utc>) {
    let rect = ui.max_rect();
    let painter = ui.painter();
    let y = rect.center().y;

    let percent = session_percent(snapshot);
    let percent_font = FontId::proportional(11.0);
    let percent_width = text_width(painter, &percent, percent_font.clone());

    let rest = flagged_summary(snapshot, now);
    let rest_font = FontId::proportional(9.5);
    let rest = if rest.is_empty() {
        String::new()
    } else {
        format!("·  {rest}")
    };
    let rest_width = if rest.is_empty() {
        0.0
    } else {
        text_width(painter, &rest, rest_font.clone()) + 7.0
    };

    let left = rect.center().x - (percent_width + rest_width) / 2.0;
    painter.text(
        Pos2::new(left, y),
        egui::Align2::LEFT_CENTER,
        percent,
        percent_font,
        rgb(visuals::session_color(snapshot)),
    );

    if !rest.is_empty() {
        painter.text(
            Pos2::new(left + percent_width + 7.0, y),
            egui::Align2::LEFT_CENTER,
            rest,
            rest_font,
            dim(0xAA),
        );
    }
}

/// Rendered width of a run, for faces that lay text out by hand.
fn text_width(painter: &egui::Painter, text: &str, font: FontId) -> f32 {
    painter
        .layout_no_wrap(text.to_owned(), font, Color32::WHITE)
        .rect
        .width()
}

/// Whether the snapshot has numbers worth drawing in colour.
fn is_live(snapshot: &UsageSnapshot) -> bool {
    matches!(snapshot.state, UsageState::Ok | UsageState::NoActiveSession)
}

/// The session percent, or an em dash when there is nothing to report.
fn session_percent(snapshot: &UsageSnapshot) -> String {
    if is_live(snapshot) {
        visuals::format_percent(snapshot.session)
    } else {
        "—".to_owned()
    }
}

/// The weekly figure and the time left, either half dropped when unknown rather than
/// rendered as a dash.
fn weekly_and_time(snapshot: &UsageSnapshot, now: DateTime<Utc>) -> String {
    let mut parts: Vec<String> = Vec::with_capacity(2);

    if let Some(weekly) = snapshot.weekly {
        parts.push(format!("wk {}", visuals::format_percent(Some(weekly))));
    }
    if snapshot.reset_at().is_some() {
        parts.push(format!(
            "{} left",
            visuals::format_duration(snapshot.time_until_reset(now))
        ));
    }

    parts.join("  ·  ")
}

/// The time left, behind the source marker when there is one, for the faces that show
/// the weekly figure elsewhere.
///
/// The marker is not optional decoration. A local estimate is measured against a
/// placeholder token budget and can read anything at all -- 210% is a perfectly
/// ordinary value for it -- so a face that prints that number without saying it is an
/// estimate is stating a falsehood. Only the rings and minimal faces have a caption
/// slot of their own; the rest have to carry it here.
fn footer_line(snapshot: &UsageSnapshot, now: DateTime<Utc>) -> String {
    let mut parts: Vec<String> = Vec::with_capacity(2);

    let caption = caption(snapshot, now);
    if !caption.is_empty() {
        parts.push(caption);
    }
    if snapshot.reset_at().is_some() {
        parts.push(format!(
            "{} left",
            visuals::format_duration(snapshot.time_until_reset(now))
        ));
    }

    parts.join(" · ")
}

/// [`weekly_and_time`] behind the source marker, for the text face -- the one face
/// with no second line and no caption slot to put it in.
fn flagged_summary(snapshot: &UsageSnapshot, now: DateTime<Utc>) -> String {
    let caption = caption(snapshot, now);
    let rest = weekly_and_time(snapshot, now);

    match (caption.is_empty(), rest.is_empty()) {
        (true, _) => rest,
        (false, true) => caption,
        (false, false) => format!("{caption} · {rest}"),
    }
}

/// The caption shown under the percent. Blank when everything is healthy and official
/// — a minimal face should say nothing when there is nothing to flag.
fn caption(snapshot: &UsageSnapshot, now: DateTime<Utc>) -> String {
    if snapshot.state == UsageState::Ok {
        if let Some(last) = snapshot.last_activity {
            if now - last > Duration::minutes(STALE_AFTER_MINUTES) {
                return "stale".to_owned();
            }
        }
    }
    visuals::state_caption(snapshot).to_owned()
}

pub fn rgb(color: [u8; 3]) -> Color32 {
    Color32::from_rgb(color[0], color[1], color[2])
}

/// White at the given alpha — the faces' one text colour, so they stay consistent.
fn dim(alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(255, 255, 255, alpha)
}

fn track() -> Color32 {
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
