use std::ops::RangeInclusive;
use std::sync::Arc;

use glam::Vec2;

use crate::color::Color;
use crate::edit::Outcome;
use crate::geometry::{Corners, Rect};
use crate::icon::{self, Icon};
use crate::input::{Button, Key};
use crate::motion::Motion;
use crate::registry::Cursor;
use crate::style::{Border, Fill, Shadow};
use crate::text::{Align, TextLayout, TextStyle};
use crate::theme::{Level, Role, TypeStyle};
use crate::ui::{Block, Length, Response, Ui};

fn knob_shadow() -> Shadow {
    Shadow::new(Color::rgba(0.0, 0.0, 0.0, 0.35), 10.0, 2.0)
}

fn button_font(ui: &Ui) -> TypeStyle {
    ui.theme().button_font()
}

fn inset(ui: &Ui) -> f32 {
    ui.theme().metrics.radius + 2.0
}

pub fn button(ui: &mut Ui, id: &str, label: &str, role: Role) -> bool {
    button_with(ui, id, label, role, Length::Auto).clicked
}

pub fn button_with(ui: &mut Ui, id: &str, label: &str, role: Role, width: Length) -> Response {
    let key = ui.id(id);
    let metrics = ui.theme().metrics;
    let font = button_font(ui);
    let measured = ui.measure_text(label, TextStyle::new(font, Color::WHITE));
    let width = match width {
        Length::Auto => measured.x + 20.0,
        Length::Fill => ui.available().x,
        Length::Fixed(value) => value,
        Length::Minimum(minimum) => (measured.x + 20.0).max(minimum),
        Length::Maximum(maximum) => (measured.x + 20.0).min(maximum),
    };
    let rect = ui.allocate(Vec2::new(width, metrics.control_height));
    let mut response = ui.interact(key, rect);
    let corners = ui.radius();
    if ui.focusable(key, rect, corners) && ui.activated(key) {
        response.clicked = true;
    }
    let press = ui.animate(
        key,
        if response.active { 1.0 } else { 0.0 },
        Motion::quick(),
    );
    let look = ui.look(key, role, &response);
    let body = rect.shrink(0.5 * press);
    let style = TextStyle::new(font, look.text).aligned(Align::Center);
    let mut painter = ui.painter();
    painter.outlined(body, look.corners, look.fill, look.border, look.shadow);
    painter.text_in(body, label, style, Align::Center);
    response
}

pub fn action_button(ui: &mut Ui, id: &str, symbol: Icon, label: &str, role: Role) -> Response {
    let key = ui.id(id);
    let metrics = ui.theme().metrics;
    let font = button_font(ui);
    let measured = ui.measure_text(label, TextStyle::new(font, Color::WHITE));
    let width = measured.x + 44.0;
    let rect = ui.allocate(Vec2::new(width, metrics.control_height));
    let mut response = ui.interact(key, rect);
    let corners = ui.radius();
    if ui.focusable(key, rect, corners) && ui.activated(key) {
        response.clicked = true;
    }
    let press = ui.animate(
        key,
        if response.active { 1.0 } else { 0.0 },
        Motion::quick(),
    );
    let look = ui.look(key, role, &response);
    let body = rect.shrink(0.5 * press);
    let leading = inset(ui);
    let glyph = Rect::from_center_size(
        Vec2::new(body.min.x + leading, body.center().y),
        Vec2::splat(14.0),
    );
    let mut painter = ui.painter();
    painter.outlined(body, look.corners, look.fill, look.border, look.shadow);
    icon::paint(symbol, &mut painter, glyph.center(), 14.0, look.text, 1.5);
    painter.text_in(
        Rect::from_min_max(
            Vec2::new(body.min.x + leading + 11.0, body.min.y),
            Vec2::new(body.max.x - 3.0, body.max.y),
        ),
        label,
        TextStyle::new(font, look.text).aligned(Align::Start),
        Align::Center,
    );
    response
}

pub fn text_button(ui: &mut Ui, id: &str, label: &str) -> bool {
    button(ui, id, label, Role::Quiet)
}

pub fn icon_button(
    ui: &mut Ui,
    id: &str,
    symbol: Icon,
    size: f32,
    role: Role,
    tooltip: &str,
) -> bool {
    icon_button_response(ui, id, symbol, size, role, tooltip).clicked
}

pub fn icon_button_response(
    ui: &mut Ui,
    id: &str,
    symbol: Icon,
    size: f32,
    role: Role,
    tooltip: &str,
) -> Response {
    let key = ui.id(id);
    let rect = ui.allocate(Vec2::splat(size));
    let mut response = ui.interact(key, rect);
    let corners = ui.radius();
    if ui.focusable(key, rect, corners) && ui.activated(key) {
        response.clicked = true;
    }
    if response.hovered {
        ui.tooltip(tooltip);
    }
    let press = ui.animate(
        key,
        if response.active { 1.0 } else { 0.0 },
        Motion::quick(),
    );
    let look = ui.look(key, role, &response);
    let body = rect.shrink(0.5 * press);
    let mut painter = ui.painter();
    painter.outlined(body, look.corners, look.fill, look.border, look.shadow);
    icon::paint(
        symbol,
        &mut painter,
        body.center(),
        (size * 0.5).clamp(11.0, 16.0),
        look.text,
        1.6,
    );
    response
}

pub fn icon_button_enabled(
    ui: &mut Ui,
    id: &str,
    symbol: Icon,
    size: f32,
    role: Role,
    tooltip: &str,
    enabled: bool,
) -> bool {
    if !enabled {
        let rect = ui.allocate(Vec2::splat(size));
        let look = ui.look_disabled(role);
        let mut painter = ui.painter();
        painter.outlined(rect, look.corners, look.fill, look.border, look.shadow);
        icon::paint(
            symbol,
            &mut painter,
            rect.center(),
            (size * 0.5).clamp(11.0, 16.0),
            look.text,
            1.6,
        );
        return false;
    }
    icon_button_response(ui, id, symbol, size, role, tooltip).clicked
}

pub fn badge(ui: &mut Ui, label: &str, tint: Color) -> Rect {
    let font = ui.theme().font(Level::Micro);
    let style = TextStyle::new(font, tint);
    let measured = ui.measure_text(label, style);
    let rect = ui.allocate(Vec2::new(measured.x + 10.0, 18.0));
    let hairline = ui.theme().metrics.hairline;
    let mut painter = ui.painter();
    painter.outlined(
        rect,
        Corners::all(4.0),
        Fill::Solid(tint.with_alpha(0.09)),
        Border {
            width: hairline,
            color: tint.with_alpha(0.18),
        },
        Shadow::NONE,
    );
    painter.text_in(rect, label, style, Align::Center);
    rect
}

pub fn toggle(ui: &mut Ui, id: &str, value: bool) -> Option<bool> {
    let key = ui.id(id);
    let rect = ui.allocate(Vec2::new(32.0, 18.0));
    let mut response = ui.interact(key, rect);
    if ui.focusable(key, rect, Corners::capsule()) && ui.activated(key) {
        response.clicked = true;
    }
    let changed = response.clicked.then_some(!value);
    let on = ui.animate(key, if value { 1.0 } else { 0.0 }, Motion::snappy());
    let palette = ui.theme().palette;
    let track = rect.shrink(1.0);
    let track_color = palette.border.lerp(palette.accent, on);
    let knob_position = track.min.x + 8.0 + (track.width() - 16.0) * on;
    let knob = Rect::from_center_size(
        Vec2::new(knob_position, track.center().y),
        Vec2::splat(14.0),
    );
    let mut painter = ui.painter();
    painter.rect(track, Corners::capsule(), track_color);
    painter.shadow(knob, Corners::capsule(), knob_shadow());
    painter.rect(
        knob,
        Corners::capsule(),
        palette
            .scene_backdrop
            .lerp(palette.text_quaternary, 1.0 - on),
    );
    changed
}

pub fn toggle_row(ui: &mut Ui, id: &str, label: &str, value: bool) -> Option<bool> {
    let key = ui.id(id);
    let height = 28.0;
    let rect = ui.allocate(Vec2::new(ui.available().x, height));
    let control = Rect::from_center_size(
        Vec2::new(rect.max.x - 16.0, rect.center().y),
        Vec2::new(32.0, 18.0),
    );
    let mut response = ui.interact(key, rect);
    let corners = Corners::all(ui.theme().metrics.radius - 2.0);
    if ui.focusable(key, rect, corners) && ui.activated(key) {
        response.clicked = true;
    }
    let changed = response.clicked.then_some(!value);
    let on = ui.animate(key, if value { 1.0 } else { 0.0 }, Motion::snappy());
    let hover = ui.animate(
        key ^ 0x33,
        if response.hovered { 1.0 } else { 0.0 },
        Motion::quick(),
    );
    let palette = ui.theme().palette;
    let track = control.shrink(1.0);
    let track_color = palette.border.lerp(palette.accent, on);
    let knob_position = track.min.x + 8.0 + (track.width() - 16.0) * on;
    let knob = Rect::from_center_size(
        Vec2::new(knob_position, track.center().y),
        Vec2::splat(14.0),
    );
    let font = ui.theme().font(Level::Callout);
    let color = palette.text_secondary.lerp(palette.text_primary, hover);
    let mut painter = ui.painter();
    painter.text_in(
        Rect::from_min_max(rect.min, Vec2::new(control.min.x - 8.0, rect.max.y)),
        label,
        TextStyle::new(font, color),
        Align::Center,
    );
    painter.rect(track, Corners::capsule(), track_color);
    painter.shadow(knob, Corners::capsule(), knob_shadow());
    painter.rect(
        knob,
        Corners::capsule(),
        palette
            .scene_backdrop
            .lerp(palette.text_quaternary, 1.0 - on),
    );
    changed
}

pub fn checkbox(ui: &mut Ui, id: &str, label: &str, value: bool) -> Option<bool> {
    let key = ui.id(id);
    let font = ui.theme().font(Level::Callout);
    let measured = ui.measure_text(label, TextStyle::new(font, Color::WHITE));
    let rect = ui.allocate(Vec2::new(measured.x + 22.0, 22.0));
    let mut response = ui.interact(key, rect);
    let corners = Corners::all(ui.theme().metrics.radius - 2.0);
    if ui.focusable(key, rect, corners) && ui.activated(key) {
        response.clicked = true;
    }
    let changed = response.clicked.then_some(!value);
    let mark = ui.animate(key, if value { 1.0 } else { 0.0 }, Motion::snappy());
    let hover = ui.animate(
        key ^ 0x11,
        if response.hovered { 1.0 } else { 0.0 },
        Motion::quick(),
    );
    let box_rect = Rect::from_center_size(
        Vec2::new(rect.min.x + 7.5, rect.center().y),
        Vec2::splat(14.0),
    );
    let palette = ui.theme().palette;
    let outline = palette.text_tertiary.lerp(palette.text_secondary, hover);
    let fill = Color::TRANSPARENT.lerp(palette.accent, mark);
    let text_color = palette.text_secondary.lerp(palette.text_primary, hover);
    let mut painter = ui.painter();
    painter.outlined(
        box_rect,
        Corners::all(3.0),
        Fill::Solid(fill),
        Border {
            width: 1.0,
            color: outline.lerp(palette.accent, mark),
        },
        Shadow::NONE,
    );
    if mark > 0.02 {
        let color = palette.text_on_accent.with_alpha(mark);
        painter.segment(
            Vec2::new(box_rect.min.x + 3.4, box_rect.min.y + 7.2),
            Vec2::new(box_rect.min.x + 5.9, box_rect.max.y - 3.8),
            1.6,
            color,
        );
        painter.segment(
            Vec2::new(box_rect.min.x + 5.9, box_rect.max.y - 3.8),
            Vec2::new(box_rect.max.x - 3.0, box_rect.min.y + 3.8),
            1.6,
            color,
        );
    }
    painter.text_in(
        Rect::from_min_max(Vec2::new(rect.min.x + 19.0, rect.min.y), rect.max),
        label,
        TextStyle::new(font, text_color),
        Align::Center,
    );
    changed
}

pub fn slider(
    ui: &mut Ui,
    id: &str,
    label: Option<&str>,
    value: f32,
    range: RangeInclusive<f32>,
    logarithmic: bool,
) -> Option<f32> {
    slider_core(
        ui,
        id,
        value,
        range,
        logarithmic,
        Length::Fill,
        SliderText {
            label,
            readout: None,
        },
    )
}

pub fn slider_sized(
    ui: &mut Ui,
    id: &str,
    label: Option<&str>,
    value: f32,
    range: RangeInclusive<f32>,
    logarithmic: bool,
    width: Length,
) -> Option<f32> {
    slider_core(
        ui,
        id,
        value,
        range,
        logarithmic,
        width,
        SliderText {
            label,
            readout: None,
        },
    )
}

pub fn slider_readout(
    ui: &mut Ui,
    id: &str,
    label: Option<&str>,
    value: f32,
    range: RangeInclusive<f32>,
    logarithmic: bool,
    readout: &str,
) -> Option<f32> {
    slider_core(
        ui,
        id,
        value,
        range,
        logarithmic,
        Length::Fill,
        SliderText {
            label,
            readout: Some(readout),
        },
    )
}

pub struct SliderText<'a> {
    pub label: Option<&'a str>,
    pub readout: Option<&'a str>,
}

fn slider_core(
    ui: &mut Ui,
    id: &str,
    value: f32,
    range: RangeInclusive<f32>,
    logarithmic: bool,
    width: Length,
    text: SliderText<'_>,
) -> Option<f32> {
    let label = text.label;
    let readout = text.readout;
    let key = ui.id(id);
    let metrics = ui.theme().metrics;
    let palette = ui.theme().palette;
    let caption = TextStyle::new(ui.theme().label_font(), palette.text_tertiary);
    let label_width = label.map_or(0.0, |text| {
        ui.measure_text(text, caption).x.min(104.0) + 10.0
    });
    let rect = ui.allocate_sized(width, metrics.control_height);
    let readout_width = if readout.is_some() { 52.0 } else { 0.0 };
    let track = Rect::from_min_max(
        Vec2::new(rect.min.x + label_width, rect.center().y - 12.0),
        Vec2::new(rect.max.x - readout_width, rect.center().y + 12.0),
    );
    let response = ui.interact(key, track);
    let (minimum, maximum) = (*range.start(), *range.end());
    let normalize = |value: f32| -> f32 {
        if logarithmic && minimum > 0.0 {
            ((value.max(minimum).ln() - minimum.ln()) / (maximum.ln() - minimum.ln()))
                .clamp(0.0, 1.0)
        } else {
            ((value - minimum) / (maximum - minimum)).clamp(0.0, 1.0)
        }
    };
    let denormalize = |t: f32| -> f32 {
        let t = t.clamp(0.0, 1.0);
        if logarithmic && minimum > 0.0 {
            (minimum.ln() + t * (maximum.ln() - minimum.ln())).exp()
        } else {
            minimum + t * (maximum - minimum)
        }
    };
    let mut changed = None;
    if response.active {
        let pointer = ui.input().position().x;
        let next = denormalize((pointer - track.min.x) / track.width().max(1.0));
        if (next - value).abs() > 1.0e-6 {
            changed = Some(next);
        }
    }
    if response.hovered && ui.input().wheel().y.abs() > 0.0 && !ui.input().modifiers().control {
        let step = (maximum - minimum) * 0.03;
        changed = Some((value + ui.input().wheel().y.signum() * step).clamp(minimum, maximum));
    }
    if ui.focusable(key, track, Corners::all(metrics.radius)) {
        let step = (maximum - minimum)
            * if ui.input().modifiers().shift {
                0.1
            } else {
                0.01
            };
        for event in ui.input().keys() {
            if !event.pressed || event.repeat {
                continue;
            }
            let next = match event.key {
                Some(Key::Left) => Some(value - step),
                Some(Key::Right) => Some(value + step),
                Some(Key::Home) => Some(minimum),
                Some(Key::End) => Some(maximum),
                _ => None,
            };
            if let Some(next) = next {
                changed = Some(next.clamp(minimum, maximum));
            }
        }
    }
    let shown = changed.unwrap_or(value);
    let target = normalize(shown);
    let position = ui.spring_to(key, target, Motion::quick());
    let engaged = ui.animate(
        key ^ 0x51,
        if response.hovered || response.active {
            1.0
        } else {
            0.0
        },
        Motion::quick(),
    );
    let bar = Rect::from_center_size(
        Vec2::new(track.center().x, rect.center().y),
        Vec2::new(track.width(), 4.0),
    );
    let knob_center = Vec2::new(bar.min.x + bar.width() * position, bar.center().y);
    let knob = Rect::from_center_size(knob_center, Vec2::splat(12.0 + engaged));
    let filled = Rect::from_min_max(
        bar.min,
        Vec2::new((knob_center.x - 2.0).max(bar.min.x), bar.max.y),
    );
    let readout_style = TextStyle::new(ui.theme().font(Level::Monospace), palette.text_secondary)
        .aligned(Align::End);
    let mut painter = ui.painter();
    if let Some(text) = label {
        painter.text_in(
            Rect::from_min_max(
                Vec2::new(rect.min.x, rect.min.y),
                Vec2::new(rect.min.x + label_width - 8.0, rect.max.y),
            ),
            text,
            caption,
            Align::Center,
        );
    }
    painter.rect(bar, Corners::capsule(), palette.border);
    if filled.width() > 0.0 {
        painter.rect(filled, Corners::capsule(), palette.accent);
    }
    painter.rect(knob, Corners::capsule(), palette.scene_backdrop);
    painter.outlined(
        knob,
        Corners::capsule(),
        Fill::None,
        Border {
            width: 1.5,
            color: palette.accent.with_alpha(0.85),
        },
        Shadow::NONE,
    );
    if let Some(text) = readout {
        painter.text_in(
            Rect::from_min_max(
                Vec2::new(rect.max.x - readout_width + 4.0, rect.min.y),
                rect.max,
            ),
            text,
            readout_style,
            Align::Center,
        );
    }
    changed
}

pub fn range_slider(
    ui: &mut Ui,
    id: &str,
    start: f32,
    end: f32,
    range: RangeInclusive<f32>,
    gradient: &[Color],
) -> Option<(f32, f32)> {
    let key = ui.id(id);
    let palette = ui.theme().palette;
    let rect = ui.allocate_full(30.0);
    let (minimum, maximum) = (*range.start(), *range.end());
    let span = (maximum - minimum).max(1.0e-6);
    let bar = Rect::from_min_max(
        Vec2::new(rect.min.x + 6.0, rect.center().y - 4.0),
        Vec2::new(rect.max.x - 6.0, rect.center().y + 4.0),
    );
    let normalize = |value: f32| ((value - minimum) / span).clamp(0.0, 1.0);
    let denormalize = |value: f32| minimum + value.clamp(0.0, 1.0) * span;
    let low_key = key ^ 0x21;
    let high_key = key ^ 0x43;
    let low_x = bar.min.x + bar.width() * normalize(start);
    let high_x = bar.min.x + bar.width() * normalize(end);
    let low_rect = Rect::from_center_size(Vec2::new(low_x, bar.center().y), Vec2::new(26.0, 28.0));
    let high_rect =
        Rect::from_center_size(Vec2::new(high_x, bar.center().y), Vec2::new(26.0, 28.0));
    let low = ui.interact(low_key, low_rect);
    let high = ui.interact(high_key, high_rect);
    let mut next_start = start;
    let mut next_end = end;
    let cursor = ui.input().position().x;
    if low.active {
        next_start =
            denormalize((cursor - bar.min.x) / bar.width().max(1.0)).min(next_end - 1.0e-4);
    }
    if high.active {
        next_end =
            denormalize((cursor - bar.min.x) / bar.width().max(1.0)).max(next_start + 1.0e-4);
    }
    if low.dragging || high.dragging {
        ui.set_cursor(Cursor::ResizeHorizontal);
    }
    let engaged = low.hovered || high.hovered || low.active || high.active;
    let grow = ui.animate(key, if engaged { 1.0 } else { 0.0 }, Motion::quick());
    if gradient.len() > 1 {
        gradient_in(ui, bar, gradient);
    }
    let low_x = bar.min.x + bar.width() * normalize(next_start);
    let high_x = bar.min.x + bar.width() * normalize(next_end);
    let low_knob = Rect::from_center_size(Vec2::new(low_x, bar.center().y), Vec2::splat(12.0));
    let high_knob =
        Rect::from_center_size(Vec2::new(high_x, bar.center().y), Vec2::splat(12.0 + grow));
    {
        let mut painter = ui.painter();
        painter.outlined(
            bar,
            Corners::capsule(),
            Fill::None,
            Border {
                width: 1.0,
                color: Color::white_alpha(0.12),
            },
            Shadow::NONE,
        );
        if low_x > bar.min.x {
            painter.rect(
                Rect::from_min_max(bar.min, Vec2::new(low_x, bar.max.y)),
                Corners::capsule(),
                Color::black_alpha(0.55),
            );
        }
        if high_x < bar.max.x {
            painter.rect(
                Rect::from_min_max(Vec2::new(high_x, bar.min.y), bar.max),
                Corners::capsule(),
                Color::black_alpha(0.55),
            );
        }
        for knob in [low_knob, high_knob] {
            painter.rect(knob, Corners::capsule(), palette.scene_backdrop);
            painter.outlined(
                knob,
                Corners::capsule(),
                Fill::None,
                Border {
                    width: 1.5,
                    color: palette.accent.with_alpha(0.9),
                },
                Shadow::NONE,
            );
        }
    }
    let changed = (next_start - start).abs() > 1.0e-6 || (next_end - end).abs() > 1.0e-6;
    changed.then_some((
        next_start.clamp(minimum, maximum),
        next_end.clamp(minimum, maximum),
    ))
}

pub fn slider_compact(
    ui: &mut Ui,
    id: &str,
    value: f32,
    range: RangeInclusive<f32>,
) -> Option<f32> {
    slider(ui, id, None, value, range, false)
}

pub fn drag_value(ui: &mut Ui, id: &str, value: f32, speed: f32, decimals: usize) -> Option<f32> {
    let key = ui.id(id);
    let metrics = ui.theme().metrics;
    let rect = ui.allocate(Vec2::new(64.0, metrics.control_height));
    let response = ui.interact(key, rect);
    let mut changed = None;
    if response.dragging {
        let delta = ui.input().delta().x;
        if delta.abs() > 0.0 {
            let scale = if ui.input().modifiers().shift {
                0.1
            } else {
                1.0
            };
            changed = Some(value + delta * speed * scale);
            ui.set_cursor(Cursor::ResizeHorizontal);
        }
    }
    if response.hovered && ui.input().wheel().y.abs() > 0.0 {
        changed = Some(value + ui.input().wheel().y * speed * 8.0);
    }
    let look = ui.look(key, Role::Field, &response);
    let font = ui.theme().font(Level::Monospace);
    let style = TextStyle::new(font, look.text).aligned(Align::End);
    let text = format!("{:.*}", decimals, changed.unwrap_or(value));
    let mut painter = ui.painter();
    painter.outlined(rect, look.corners, look.fill, look.border, look.shadow);
    painter.text_in(
        rect.shrink2(Vec2::new(8.0, 0.0)),
        &text,
        style,
        Align::Center,
    );
    changed
}

pub fn segmented(ui: &mut Ui, id: &str, options: &[&str], selected: usize) -> Option<usize> {
    let key = ui.id(id);
    let metrics = ui.theme().metrics;
    let palette = ui.theme().palette;
    let font = ui.theme().font(Level::Callout);
    let style = TextStyle::new(font, Color::WHITE);
    let widths = options
        .iter()
        .map(|option| ui.measure_text(option, style).x + 18.0)
        .collect::<Vec<_>>();
    let total = widths.iter().sum::<f32>() + 4.0;
    let rect = ui.allocate(Vec2::new(total, metrics.control_height));
    let response = ui.interact(key, rect);
    let mut changed = None;
    let mut left = rect.min.x + 2.0;
    let mut cells = Vec::new();
    for (index, width) in widths.iter().enumerate() {
        let cell = Rect::from_min_size(
            Vec2::new(left, rect.min.y + 2.0),
            Vec2::new(*width, rect.height() - 4.0),
        );
        cells.push(cell);
        let cell_key = key ^ (index as u64 + 1).wrapping_mul(0x9e37_79b9_7f4a_7c15);
        let response = ui.interact(cell_key, cell);
        let corners = Corners::all(metrics.radius - 2.0);
        if ui.focusable(cell_key, cell, corners) && ui.activated(cell_key) {
            changed = Some(index);
        }
        if response.clicked {
            changed = Some(index);
        }
        left += width;
    }
    let target = selected.min(options.len().saturating_sub(1));
    let target_x = cells.get(target).map_or(rect.min.x, |cell| cell.min.x);
    let animated = ui.spring_to(key, target_x, Motion::snappy());
    let width = cells.get(target).map_or(0.0, |cell| cell.width());
    let pill = Rect::from_min_size(
        Vec2::new(animated, rect.min.y + 2.0),
        Vec2::new(width.max(0.0), rect.height() - 4.0),
    );
    let pointer = ui.input().position();
    let track_fill = if ui.theme().dark {
        palette.card
    } else {
        Color::bytes(244, 244, 245)
    };
    let mut painter = ui.painter();
    painter.outlined(
        rect,
        Corners::all(metrics.radius),
        Fill::Solid(track_fill),
        Border {
            width: metrics.hairline,
            color: palette.border,
        },
        Shadow::NONE,
    );
    if !response.active {
        for (index, cell) in cells.iter().enumerate() {
            if cell.contains(pointer) && index != target {
                painter.rect(
                    *cell,
                    Corners::all(metrics.radius - 2.0),
                    palette.raised.with_alpha(0.8),
                );
            }
        }
    }
    painter.outlined(
        pill,
        Corners::all(metrics.radius - 2.0),
        Fill::Solid(palette.selected),
        Border {
            width: metrics.hairline,
            color: Color::bytes(63, 63, 70),
        },
        Shadow::NONE,
    );
    for (index, cell) in cells.iter().enumerate() {
        let color = if index == target {
            palette.text_primary
        } else {
            palette.text_tertiary
        };
        let style = TextStyle::new(font, color).aligned(Align::Center);
        painter.text_in(*cell, options[index], style, Align::Center);
    }
    changed
}

pub fn tabs(ui: &mut Ui, id: &str, options: &[&str], selected: usize) -> Option<usize> {
    tab_bar(ui, id, options, selected, false)
}

pub fn tab_bar(
    ui: &mut Ui,
    id: &str,
    options: &[&str],
    selected: usize,
    bordered: bool,
) -> Option<usize> {
    let palette = ui.theme().palette;
    let metrics = ui.theme().metrics;
    let font = ui.theme().font(Level::Callout).medium();
    let style = TextStyle::new(font, Color::WHITE);
    let height = 34.0;
    let available = ui.available().x;
    let widths = options
        .iter()
        .map(|option| ui.measure_text(option, style).x + 24.0)
        .collect::<Vec<_>>();
    let rect = ui.allocate(Vec2::new(available, height));
    if bordered {
        let mut painter = ui.painter();
        painter.rect(
            Rect::from_min_size(
                Vec2::new(rect.min.x, rect.max.y - metrics.hairline),
                Vec2::new(rect.width(), metrics.hairline),
            ),
            Corners::ZERO,
            palette.separator,
        );
    }
    let mut changed = None;
    let mut left = rect.min.x;
    for (index, width) in widths.iter().enumerate() {
        let cell = Rect::from_min_size(Vec2::new(left, rect.min.y), Vec2::new(*width, height));
        let key = ui.id(&format!("{id}.tab.{index}"));
        let response = ui.interact(key, cell);
        let corners = Corners::all(ui.theme().metrics.radius - 2.0);
        if ui.focusable(key, cell, corners) && ui.activated(key) {
            changed = Some(index);
        }
        if response.clicked {
            changed = Some(index);
        }
        let hover = ui.animate(
            ui.id(&format!("{id}.tab.{index}.hover")),
            if response.hovered { 1.0 } else { 0.0 },
            Motion::quick(),
        );
        let active = index == selected;
        let color = if active {
            palette.text_primary
        } else {
            palette.text_tertiary.lerp(palette.text_secondary, hover)
        };
        let mut painter = ui.painter();
        painter.text_in(
            cell,
            options[index],
            TextStyle::new(font, color).aligned(Align::Center),
            Align::Center,
        );
        if active {
            painter.rect(
                Rect::from_min_size(
                    Vec2::new(cell.min.x + 8.0, cell.max.y - 2.0),
                    Vec2::new((cell.width() - 16.0).max(0.0), 2.0),
                ),
                Corners::all(1.0),
                palette.accent,
            );
        }
        left += width;
    }
    changed
}

pub fn disclosure(
    ui: &mut Ui,
    id: &str,
    title: &str,
    open: bool,
    content: impl FnOnce(&mut Ui),
) -> bool {
    let key = ui.id(id);
    let palette = ui.theme().palette;
    let header = ui.allocate_full(28.0);
    let mut response = ui.interact(key, header);
    let corners = Corners::all(ui.theme().metrics.radius - 2.0);
    if ui.focusable(key, header, corners) && ui.activated(key) {
        response.clicked = true;
    }
    let next = if response.clicked { !open } else { open };
    let arrow = ui.animate(key, if open { 1.0 } else { 0.0 }, Motion::snappy());
    let font = ui.theme().font(Level::Callout).medium();
    let hover = ui.animate(
        key ^ 0x31,
        if response.hovered { 1.0 } else { 0.0 },
        Motion::quick(),
    );
    let color = palette.text_secondary.lerp(palette.text_primary, hover);
    let style = TextStyle::new(font, color);
    let radius = ui.theme().metrics.radius - 2.0;
    {
        let mut painter = ui.painter();
        if hover > 0.01 {
            painter.rect(
                header.shrink(1.0),
                Corners::all(radius),
                palette.raised.with_alpha(0.7 * hover),
            );
        }
        painter.text_in(
            Rect::from_min_max(Vec2::new(header.min.x + 20.0, header.min.y), header.max),
            title,
            style,
            Align::Center,
        );
        let centre = Vec2::new(header.min.x + 7.0, header.center().y);
        let angle = std::f32::consts::FRAC_PI_2 * (0.5 + 0.5 * arrow);
        let tip = centre + Vec2::new(angle.cos(), angle.sin()) * 2.2;
        painter.segment(tip + Vec2::new(-2.0, -1.3), tip, 1.4, palette.text_tertiary);
        painter.segment(tip, tip + Vec2::new(2.0, -1.3), 1.4, palette.text_tertiary);
    }
    if next {
        ui.column(Block::column().gaps(4.0), content);
    }
    next
}

pub fn spinner(ui: &mut Ui, size: f32) {
    let rect = ui.allocate(Vec2::splat(size));
    let time = ui.time();
    let color = ui.theme().palette.text_tertiary;
    let center = rect.center();
    let mut painter = ui.painter();
    painter.ring(
        center,
        size * 0.36,
        1.5,
        0.0,
        std::f32::consts::TAU,
        color.with_alpha(0.18),
    );
    painter.ring(center, size * 0.36, 1.5, time * 5.0, 1.7, color);
}

pub fn progress(ui: &mut Ui, fraction: f32, width: f32) {
    let palette = ui.theme().palette;
    let rect = ui.allocate(Vec2::new(width, 4.0));
    let mut painter = ui.painter();
    painter.rect(rect, Corners::capsule(), palette.border);
    let width = rect.width() * fraction.clamp(0.0, 1.0);
    if width > 1.0 {
        painter.rect(
            Rect::from_min_size(rect.min, Vec2::new(width, rect.height())),
            Corners::capsule(),
            palette.accent,
        );
    }
}

pub fn color_swatch(ui: &mut Ui, id: &str, color: [f32; 3], size: f32) -> bool {
    let key = ui.id(id);
    let rect = ui.allocate(Vec2::splat(size));
    let response = ui.interact(key, rect);
    let mut painter = ui.painter();
    painter.rect(rect, Corners::all(4.0), Color::from_rgb_f32(color));
    painter.outlined(
        rect,
        Corners::all(4.0),
        Fill::None,
        Border {
            width: 1.0,
            color: Color::white_alpha(if response.hovered { 0.55 } else { 0.22 }),
        },
        Shadow::NONE,
    );
    response.clicked
}

pub fn gradient_bar(ui: &mut Ui, colors: &[Color], height: f32) {
    let rect = ui.allocate_full(height);
    gradient_in(ui, rect, colors);
}

pub fn gradient(ui: &mut Ui, colors: &[Color], height: f32) {
    let rect = ui.allocate_full(height);
    gradient_in(ui, rect, colors);
}

pub fn gradient_in(ui: &mut Ui, rect: Rect, colors: &[Color]) {
    if colors.len() < 2 {
        return;
    }
    let mut painter = ui.painter();
    let segments = colors.len() - 1;
    for (index, color) in colors.iter().take(segments).enumerate() {
        let left = rect.min.x + rect.width() * index as f32 / segments as f32;
        let right = rect.min.x + rect.width() * (index + 1) as f32 / segments as f32;
        painter.rect(
            Rect::from_min_max(
                Vec2::new(left, rect.min.y),
                Vec2::new(right + 0.6, rect.max.y),
            ),
            Corners::all(2.0),
            *color,
        );
    }
}

pub fn labelled(ui: &mut Ui, label: &str, value: &str) {
    let palette = ui.theme().palette;
    let name = TextStyle::new(ui.theme().label_font(), palette.text_tertiary);
    let content = TextStyle::new(ui.theme().font(Level::Monospace), palette.text_secondary);
    let width = ui.available().x;
    let label_width = (width * 0.38).clamp(64.0, 124.0);
    let rect = ui.allocate(Vec2::new(width, 22.0));
    let mut painter = ui.painter();
    painter.text_in(
        Rect::from_min_size(rect.min, Vec2::new(label_width, rect.height())),
        label,
        name,
        Align::Center,
    );
    painter.text_in(
        Rect::from_min_size(
            Vec2::new(rect.min.x + label_width, rect.min.y),
            Vec2::new(rect.width() - label_width, rect.height()),
        ),
        value,
        TextStyle::new(content.font, content.color).aligned(Align::End),
        Align::Center,
    );
}

pub fn section_title(ui: &mut Ui, title: &str) {
    let style = TextStyle::new(ui.theme().label_font(), ui.theme().palette.text_tertiary);
    let rect = ui.allocate_full(24.0);
    let mut painter = ui.painter();
    painter.text_in(rect, title, style, Align::Center);
}

pub fn caption(ui: &mut Ui, text: &str) {
    let palette = ui.theme().palette;
    let style = TextStyle::new(ui.theme().font(Level::Caption), palette.text_tertiary);
    ui.text_style(text, style);
}

pub fn weak(ui: &mut Ui, text: &str) {
    let palette = ui.theme().palette;
    let style = TextStyle::new(ui.theme().font(Level::Caption), palette.text_tertiary);
    let available = ui.available().x;
    ui.text_style_wrapped(text, style.wrapping(), available);
}

pub fn body(ui: &mut Ui, text: &str) {
    let palette = ui.theme().palette;
    let style = TextStyle::new(ui.theme().font(Level::Body), palette.text_secondary);
    ui.text_style(text, style);
}

pub fn body_wrapped(ui: &mut Ui, text: &str) {
    let palette = ui.theme().palette;
    let style = TextStyle::new(ui.theme().font(Level::Body), palette.text_secondary);
    let available = ui.available().x;
    ui.text_style_wrapped(text, style.wrapping(), available);
}

pub fn headline(ui: &mut Ui, text: &str) {
    let palette = ui.theme().palette;
    let style = TextStyle::new(ui.theme().font(Level::Headline), palette.text_primary);
    ui.text_style(text, style);
}

pub fn title(ui: &mut Ui, text: &str) {
    let palette = ui.theme().palette;
    let style = TextStyle::new(ui.theme().font(Level::Title), palette.text_primary);
    ui.text_style(text, style);
}

pub fn field_label(ui: &mut Ui, text: &str) {
    let style = TextStyle::new(ui.theme().label_font(), ui.theme().palette.text_tertiary);
    ui.text_style(text, style);
}

pub fn row(ui: &mut Ui, gap: f32, content: impl FnOnce(&mut Ui)) -> Rect {
    ui.row(Block::row().gaps(gap).align(Align::Center), content)
}

pub fn column(ui: &mut Ui, gap: f32, content: impl FnOnce(&mut Ui)) -> Rect {
    ui.column(Block::column().gaps(gap), content)
}

const FIELD_PAD: f32 = 8.0;

pub fn text_field(
    ui: &mut Ui,
    id: &str,
    rect: Rect,
    style: TextStyle,
) -> Option<(Outcome, String)> {
    let key = ui.id(id);
    if !ui.is_editing(key) {
        return None;
    }
    ui.touch_edit();
    let response = ui.interact(key, rect);
    ui.focusable(key, rect, Corners::all(ui.theme().metrics.radius));
    if response.hovered {
        ui.set_cursor(Cursor::Text);
    }
    let time = ui.time();
    let pointer = ui.input().position();
    let primary = ui.input().button(Button::Primary);
    let modifiers = ui.input().modifiers();
    let preedit = ui.input().preedit().map(str::to_string).unwrap_or_default();
    let offset = field_offset(ui, style, rect, &preedit);
    let outcome = match ui.poll_edit() {
        Some(outcome) => Some(outcome),
        None if primary.pressed => {
            if !rect.contains(pointer) {
                Some(Outcome::Commit)
            } else if preedit.is_empty() {
                let count = ui.edit_mut().map_or(1, |edit| edit.click(pointer, time));
                let caret = caret_at(ui, style, rect, offset, pointer.x);
                if let Some(edit) = ui.edit_mut() {
                    match count {
                        2 => {
                            let range = edit.word(caret);
                            edit.select(range, time);
                        }
                        3 => edit.select_all(time),
                        _ => edit.place(caret, modifiers.shift, time),
                    }
                }
                None
            } else {
                None
            }
        }
        None if primary.down && response.dragging => {
            let caret = caret_at(ui, style, rect, offset, pointer.x);
            if let Some(edit) = ui.edit_mut() {
                edit.place(caret, true, time);
            }
            None
        }
        None => None,
    };
    let snapshot = ui.edit().map(|edit| {
        (
            edit.text().to_string(),
            edit.caret(),
            edit.anchor(),
            edit.caret_visible(time),
        )
    });
    if outcome.is_some() {
        ui.end_edit();
    }
    let (text, caret, anchor, blinking) = snapshot?;
    paint_field(
        ui,
        key,
        &response,
        rect,
        style,
        Field {
            text: &text,
            caret,
            anchor,
            blinking,
            preedit: &preedit,
        },
    );
    outcome.map(|outcome| (outcome, text))
}

struct Field<'a> {
    text: &'a str,
    caret: usize,
    anchor: usize,
    blinking: bool,
    preedit: &'a str,
}

fn field_offset(ui: &mut Ui, style: TextStyle, rect: Rect, preedit: &str) -> f32 {
    let Some(edit) = ui.edit() else {
        return 0.0;
    };
    let text = edit.text().to_string();
    let composition = format!("{}{}", prefix(&text, edit.caret()), preedit);
    let caret_x = text_width(ui, style, &composition);
    let inner = rect.shrink2(Vec2::new(FIELD_PAD, 3.0)).width();
    (caret_x - inner + 2.0).max(0.0)
}

fn paint_field(
    ui: &mut Ui,
    key: u64,
    response: &Response,
    rect: Rect,
    style: TextStyle,
    field: Field<'_>,
) {
    let Field {
        text,
        caret,
        anchor,
        blinking,
        preedit,
    } = field;
    let look = ui.look(key, Role::Field, response);
    let palette = ui.theme().palette;
    let radius = ui.theme().metrics.radius - 1.0;
    let inner = rect.shrink2(Vec2::new(FIELD_PAD, 3.0));
    let offset_of = |index: usize| prefix(text, index).len();
    let display = if preedit.is_empty() {
        text.to_string()
    } else {
        let mut display = String::with_capacity(text.len() + preedit.len());
        display.push_str(prefix(text, caret));
        display.push_str(preedit);
        display.push_str(&text[offset_of(caret)..]);
        display
    };
    let composed = preedit.chars().count();
    let display_caret = caret + composed;
    let caret_x = text_width(ui, style, prefix(&display, display_caret));
    let offset = (caret_x - inner.width() + 2.0).max(0.0);
    let layout = ui.layout_text(&display, style, 0.0);
    let origin = Vec2::new(
        inner.min.x - offset,
        inner.min.y + (inner.height() - layout.size.y) * 0.5,
    );
    let height = (layout.size.y * 0.86).max(8.0);
    let top = origin.y + (layout.size.y - height) * 0.5;
    let selection = (preedit.is_empty() && caret != anchor).then(|| {
        let (start, end) = (caret.min(anchor), caret.max(anchor));
        (
            text_width(ui, style, prefix(text, start)),
            text_width(ui, style, prefix(text, end)),
        )
    });
    let composition =
        (composed > 0).then(|| (text_width(ui, style, prefix(&display, caret)), caret_x));
    let accent = palette.accent;
    let mut painter = ui.painter();
    painter.outlined(
        rect,
        Corners::all(radius),
        look.fill,
        look.border,
        Shadow::NONE,
    );
    let clip = painter.clip();
    painter.push_clip(inner, Corners::ZERO);
    if let Some((left, right)) = selection {
        painter.rect(
            Rect::from_min_max(
                Vec2::new(inner.min.x - offset + left, top),
                Vec2::new(inner.min.x - offset + right, top + height),
            ),
            Corners::ZERO,
            accent.with_alpha(0.26),
        );
    }
    if let Some((left, right)) = composition {
        painter.rect(
            Rect::from_min_max(
                Vec2::new(inner.min.x - offset + left, top + height + 2.0),
                Vec2::new(inner.min.x - offset + right, top + height + 3.2),
            ),
            Corners::ZERO,
            accent,
        );
    }
    painter.glyphs(&layout, origin, style.color);
    if blinking {
        painter.rect(
            Rect::from_min_size(
                Vec2::new(inner.min.x - offset + caret_x, top),
                Vec2::new(1.4, height),
            ),
            Corners::ZERO,
            accent,
        );
    }
    painter.pop_clip(clip);
    ui.set_edit_caret(Rect::from_min_size(
        Vec2::new(inner.min.x - offset + caret_x, top),
        Vec2::new(1.4, height),
    ));
}

pub fn text_area(ui: &mut Ui, id: &str, rect: Rect, style: TextStyle) -> Option<(Outcome, String)> {
    let key = ui.id(id);
    if !ui.is_editing(key) {
        return None;
    }
    ui.touch_edit();
    let response = ui.interact(key, rect);
    let radius = ui.theme().metrics.radius;
    ui.focusable(key, rect, Corners::all(radius));
    if response.hovered {
        ui.set_cursor(Cursor::Text);
    }
    let inner = rect.shrink2(Vec2::new(FIELD_PAD, 3.0));
    let line_height = style.font.leading.max(style.font.size * 1.28);
    let scroll_key = key ^ 0x2f6e_9a3d_1c4b_8a57;
    let mut scroll = ui.value_of(scroll_key).unwrap_or(0.0).max(0.0);
    let time = ui.time();
    let pointer = ui.input().position();
    let primary = ui.input().button(Button::Primary);
    let chars = ui
        .edit()
        .map(|edit| edit.text().chars().collect::<Vec<_>>())
        .unwrap_or_default();
    let spans = line_spans(&chars);
    if response.hovered && ui.input().wheel().y != 0.0 {
        scroll -= ui.input().wheel().y * line_height * 3.0;
    }
    let mut commit = false;
    if primary.pressed {
        if !rect.contains(pointer) {
            commit = true;
        } else {
            let caret = area_caret_at(
                ui,
                style,
                AreaText {
                    inner,
                    chars: &chars,
                    spans: &spans,
                    line_height,
                    scroll,
                },
                pointer,
            );
            let count = ui.edit_mut().map_or(1, |edit| edit.click(pointer, time));
            if let Some(edit) = ui.edit_mut() {
                match count {
                    2 => {
                        let range = edit.word(caret);
                        edit.select(range, time);
                    }
                    3 => edit.select_all(time),
                    _ => edit.place(caret, false, time),
                }
            }
        }
    } else if primary.down && response.dragging {
        let caret = area_caret_at(
            ui,
            style,
            AreaText {
                inner,
                chars: &chars,
                spans: &spans,
                line_height,
                scroll,
            },
            pointer,
        );
        if let Some(edit) = ui.edit_mut() {
            edit.place(caret, true, time);
        }
    }
    let outcome = if commit {
        Some(Outcome::Commit)
    } else {
        ui.poll_edit()
    };
    let snapshot = ui.edit().map(|edit| {
        (
            edit.text().to_string(),
            edit.caret(),
            edit.anchor(),
            edit.caret_visible(time),
        )
    });
    if outcome.is_some() {
        ui.end_edit();
    }
    let (text, caret, anchor, blinking) = snapshot?;
    let chars = text.chars().collect::<Vec<_>>();
    let spans = line_spans(&chars);
    let (caret_line, caret_x, caret_y) =
        area_caret_xy(ui, style, &chars, &spans, caret, line_height);
    let max_scroll = (spans.len() as f32 * line_height - inner.height()).max(0.0);
    if caret_y < scroll {
        scroll = caret_y;
    } else if caret_y + line_height > scroll + inner.height() {
        scroll = caret_y + line_height - inner.height();
    }
    scroll = scroll.clamp(0.0, max_scroll);
    ui.store_value(scroll_key, scroll);
    let selection = (anchor.min(caret), anchor.max(caret));
    let mut select_rects = Vec::new();
    if selection.0 != selection.1 {
        for (index, (start, end)) in spans.iter().enumerate() {
            let low = selection.0.max(*start);
            let high = selection.1.min(*end);
            if low < high {
                let x1 = line_width(ui, style, &chars, *start, low);
                let x2 = line_width(ui, style, &chars, *start, high);
                select_rects.push(Rect::from_min_size(
                    Vec2::new(
                        inner.min.x + x1,
                        inner.min.y - scroll + index as f32 * line_height,
                    ),
                    Vec2::new(x2 - x1, line_height),
                ));
            }
        }
    }
    let layouts = spans
        .iter()
        .map(|(start, end)| {
            let line = chars[*start..*end].iter().collect::<String>();
            ui.layout_text(&line, style, 0.0)
        })
        .collect::<Vec<Arc<TextLayout>>>();
    let palette = ui.theme().palette;
    let mut painter = ui.painter();
    let clip = painter.clip();
    painter.push_clip(inner, Corners::all(radius));
    for selection in select_rects {
        painter.rect(selection, Corners::ZERO, palette.accent.with_alpha(0.28));
    }
    let first = (scroll / line_height).floor().max(0.0) as usize;
    let last = ((scroll + inner.height()) / line_height).ceil() as usize + 1;
    for (index, layout) in layouts.iter().enumerate().take(last).skip(first) {
        painter.text(
            layout,
            Vec2::new(
                inner.min.x,
                inner.min.y - scroll + index as f32 * line_height,
            ),
            style.color,
        );
    }
    let caret_rect = Rect::from_min_size(
        Vec2::new(
            inner.min.x + caret_x,
            inner.min.y - scroll + caret_line as f32 * line_height + 1.0,
        ),
        Vec2::new(1.4, (line_height - 2.0).max(1.0)),
    );
    if blinking {
        painter.rect(caret_rect, Corners::ZERO, palette.accent);
    }
    painter.pop_clip(clip);
    ui.set_edit_caret(caret_rect);
    outcome.map(|outcome| (outcome, text))
}

fn line_spans(chars: &[char]) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut start = 0;
    for (index, character) in chars.iter().enumerate() {
        if *character == '\n' {
            spans.push((start, index));
            start = index + 1;
        }
    }
    spans.push((start, chars.len()));
    spans
}

fn line_width(ui: &mut Ui, style: TextStyle, chars: &[char], start: usize, end: usize) -> f32 {
    if start >= end {
        return 0.0;
    }
    let line = chars[start..end].iter().collect::<String>();
    ui.measure_text(&line, style).x
}

fn area_caret_xy(
    ui: &mut Ui,
    style: TextStyle,
    chars: &[char],
    spans: &[(usize, usize)],
    caret: usize,
    line_height: f32,
) -> (usize, f32, f32) {
    let caret = caret.min(chars.len());
    let mut line = spans.len() - 1;
    for (index, (_, end)) in spans.iter().enumerate() {
        if caret <= *end {
            line = index;
            break;
        }
    }
    let start = spans[line].0;
    let x = line_width(ui, style, chars, start, caret);
    (line, x, line as f32 * line_height)
}

struct AreaText<'a> {
    inner: Rect,
    chars: &'a [char],
    spans: &'a [(usize, usize)],
    line_height: f32,
    scroll: f32,
}

fn area_caret_at(ui: &mut Ui, style: TextStyle, area: AreaText<'_>, point: Vec2) -> usize {
    let line = (((point.y - area.inner.min.y + area.scroll) / area.line_height).floor() as isize)
        .clamp(0, area.spans.len() as isize - 1) as usize;
    let (start, end) = area.spans[line];
    let x = point.x - area.inner.min.x;
    let mut best = start;
    let mut distance = f32::INFINITY;
    for column in 0..=end - start {
        let width = line_width(ui, style, area.chars, start, start + column);
        let candidate = (width - x).abs();
        if candidate < distance {
            distance = candidate;
            best = start + column;
        }
    }
    best
}

fn caret_at(ui: &mut Ui, style: TextStyle, rect: Rect, offset: f32, x: f32) -> usize {
    let Some(edit) = ui.edit() else {
        return 0;
    };
    let text = edit.text().to_string();
    let inner = rect.shrink2(Vec2::new(FIELD_PAD, 3.0));
    let x = x - inner.min.x + offset;
    let mut best = 0usize;
    let mut distance = f32::INFINITY;
    for index in 0..=text.chars().count() {
        let width = text_width(ui, style, prefix(&text, index));
        let candidate = (width - x).abs();
        if candidate < distance {
            distance = candidate;
            best = index;
        }
    }
    best
}

fn text_width(ui: &mut Ui, style: TextStyle, text: &str) -> f32 {
    if text.is_empty() {
        return 0.0;
    }
    ui.measure_text(text, style).x
}

fn prefix(text: &str, index: usize) -> &str {
    let offset = text
        .char_indices()
        .nth(index)
        .map_or(text.len(), |(offset, _)| offset);
    &text[..offset]
}
