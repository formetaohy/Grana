use std::ops::RangeInclusive;

use glam::Vec2;

use crate::color::Color;
use crate::geometry::{Corners, Rect};
use crate::motion::Motion;
use crate::style::{Border, Fill, Shadow};
use crate::text::{Align, TextStyle};
use crate::theme::{Level, Role};
use crate::ui::{Block, Length, Response, Ui};

fn knob_shadow() -> Shadow {
    Shadow::new(Color::rgba(0.0, 0.0, 0.0, 0.30), 11.0, 2.0)
}

pub fn button(ui: &mut Ui, id: &str, label: &str, role: Role) -> bool {
    button_with(ui, id, label, role, Length::Auto).clicked
}

pub fn button_with(ui: &mut Ui, id: &str, label: &str, role: Role, width: Length) -> Response {
    let key = ui.id(id);
    let metrics = ui.theme().metrics;
    let font = ui.theme().font(Level::Body);
    let measured = ui.measure_text(label, TextStyle::new(font, Color::WHITE));
    let width = match width {
        Length::Auto => measured.x + 22.0,
        Length::Fill => ui.available().x,
        Length::Fixed(value) => value,
        Length::Maximum(maximum) => (measured.x + 22.0).min(maximum),
    };
    let rect = ui.allocate(Vec2::new(width, metrics.control_height));
    let response = ui.interact(key, rect);
    let press = ui.animate(
        key,
        if response.active { 1.0 } else { 0.0 },
        Motion::quick(),
    );
    let look = ui.look(key, role, &response);
    let body = rect.shrink(0.6 * press);
    let style = TextStyle::new(font, look.text).aligned(Align::Center);
    let mut painter = ui.painter();
    painter.outlined(body, look.corners, look.fill, look.border, look.shadow);
    painter.text_in(body, label, style, Align::Center);
    response
}

pub fn text_button(ui: &mut Ui, id: &str, label: &str) -> bool {
    button(ui, id, label, Role::Quiet)
}

pub fn icon_button(ui: &mut Ui, id: &str, symbol: &str, size: f32, role: Role) -> bool {
    let key = ui.id(id);
    let rect = ui.allocate(Vec2::splat(size));
    let response = ui.interact(key, rect);
    let look = ui.look(key, role, &response);
    let font = ui.theme().font(Level::Title);
    let style = TextStyle::new(font, look.text).aligned(Align::Center);
    let mut painter = ui.painter();
    painter.outlined(rect, look.corners, look.fill, look.border, look.shadow);
    painter.text_in(rect, symbol, style, Align::Center);
    response.clicked
}

pub fn badge(ui: &mut Ui, label: &str, tint: Color) -> Rect {
    let font = ui.theme().font(Level::Micro);
    let style = TextStyle::new(font, tint);
    let measured = ui.measure_text(label, style);
    let rect = ui.allocate(Vec2::new(measured.x + 12.0, 16.0));
    let mut painter = ui.painter();
    painter.rect(rect, Corners::capsule(), tint.with_alpha(0.16));
    painter.text_in(rect, label, style, Align::Center);
    rect
}

pub fn toggle(ui: &mut Ui, id: &str, value: bool) -> Option<bool> {
    let key = ui.id(id);
    let rect = ui.allocate(Vec2::new(36.0, 20.0));
    let response = ui.interact(key, rect);
    let changed = response.clicked.then_some(!value);
    let on = ui.animate(key, if value { 1.0 } else { 0.0 }, Motion::bouncy());
    let accent = ui.theme().palette.accent;
    let track_color = if value {
        accent
    } else {
        Color::white_alpha(if response.hovered { 0.24 } else { 0.16 })
    };
    let track = rect.shrink(1.5);
    let knob_position = track.min.x + 8.5 + (track.width() - 17.0) * on;
    let knob = Rect::from_center_size(
        Vec2::new(knob_position, track.center().y),
        Vec2::splat(17.0),
    );
    let mut painter = ui.painter();
    painter.rect(track, Corners::capsule(), track_color);
    painter.shadow(knob, Corners::capsule(), knob_shadow());
    painter.rect(knob, Corners::capsule(), Color::WHITE);
    changed
}

pub fn toggle_row(ui: &mut Ui, id: &str, label: &str, value: bool) -> Option<bool> {
    let key = ui.id(id);
    let height = 24.0;
    let rect = ui.allocate(Vec2::new(ui.available().x, height));
    let control = Rect::from_min_size(
        Vec2::new(rect.max.x - 36.0, rect.center().y - 10.0),
        Vec2::new(36.0, 20.0),
    );
    let response = ui.interact(key, rect);
    let changed = response.clicked.then_some(!value);
    let on = ui.animate(key, if value { 1.0 } else { 0.0 }, Motion::bouncy());
    let hover = ui.animate(
        key ^ 0x33,
        if response.hovered { 1.0 } else { 0.0 },
        Motion::quick(),
    );
    let accent = ui.theme().palette.accent;
    let track_color = if value {
        accent
    } else {
        Color::white_alpha(0.16 + 0.08 * hover)
    };
    let font = ui.theme().font(Level::Callout);
    let text_color = ui
        .theme()
        .text(Level::Callout)
        .lerp(ui.theme().palette.text_primary, hover);
    let track = control.shrink(1.5);
    let knob_position = track.min.x + 8.5 + (track.width() - 17.0) * on;
    let knob = Rect::from_center_size(
        Vec2::new(knob_position, track.center().y),
        Vec2::splat(17.0),
    );
    let style = TextStyle::new(font, text_color);
    let mut painter = ui.painter();
    if response.hovered {
        painter.rect(
            rect.shrink(3.0),
            Corners::all(7.0),
            Color::white_alpha(0.04),
        );
    }
    painter.text_in(
        Rect::from_min_max(rect.min, Vec2::new(control.min.x - 8.0, rect.max.y)),
        label,
        style,
        Align::Center,
    );
    painter.rect(track, Corners::capsule(), track_color);
    painter.shadow(knob, Corners::capsule(), knob_shadow());
    painter.rect(knob, Corners::capsule(), Color::WHITE);
    changed
}

pub fn checkbox(ui: &mut Ui, id: &str, label: &str, value: bool) -> Option<bool> {
    let key = ui.id(id);
    let font = ui.theme().font(Level::Callout);
    let measured = ui.measure_text(label, TextStyle::new(font, Color::WHITE));
    let rect = ui.allocate(Vec2::new(measured.x + 26.0, 22.0));
    let response = ui.interact(key, rect);
    let changed = response.clicked.then_some(!value);
    let mark = ui.animate(key, if value { 1.0 } else { 0.0 }, Motion::snappy());
    let hover = ui.animate(
        key ^ 0x11,
        if response.hovered { 1.0 } else { 0.0 },
        Motion::quick(),
    );
    let box_rect = Rect::from_center_size(
        Vec2::new(rect.min.x + 8.0, rect.center().y),
        Vec2::splat(15.0),
    );
    let accent = ui.theme().palette.accent;
    let fill = Color::white_alpha(0.08 + 0.06 * hover).lerp(accent, mark);
    let text_color = ui
        .theme()
        .text(Level::Callout)
        .lerp(ui.theme().palette.text_primary, hover);
    let style = TextStyle::new(font, text_color);
    let mut painter = ui.painter();
    painter.outlined(
        box_rect,
        Corners::all(4.5),
        Fill::Solid(fill),
        Border {
            width: 0.5,
            color: Color::white_alpha(0.14 * (1.0 - mark)),
        },
        Shadow::NONE,
    );
    if mark > 0.02 {
        let color = Color::WHITE.with_alpha(mark);
        painter.segment(
            Vec2::new(box_rect.min.x + 4.0, box_rect.min.y + 7.6),
            Vec2::new(box_rect.min.x + 6.4, box_rect.max.y - 4.4),
            1.6,
            color,
        );
        painter.segment(
            Vec2::new(box_rect.min.x + 6.4, box_rect.max.y - 4.4),
            Vec2::new(box_rect.max.x - 3.6, box_rect.min.y + 4.4),
            1.6,
            color,
        );
    }
    painter.text_in(
        Rect::from_min_max(Vec2::new(rect.min.x + 22.0, rect.min.y), rect.max),
        label,
        style,
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
    let caption = TextStyle::new(
        ui.theme().font(Level::Caption),
        ui.theme().text(Level::Caption),
    );
    let label_width = label.map_or(0.0, |text| {
        ui.measure_text(text, caption).x.min(104.0) + metrics.gap
    });
    let rect = ui.allocate_sized(width, metrics.control_height);
    let readout_width = if readout.is_some() { 64.0 } else { 0.0 };
    let track = Rect::from_min_max(
        Vec2::new(rect.min.x + label_width, rect.center().y - 10.0),
        Vec2::new(rect.max.x - readout_width, rect.center().y + 10.0),
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
    if response.active
        && let Some(pointer) = ui.pointer_in(track)
    {
        let next = denormalize((pointer.x - track.min.x) / track.width().max(1.0));
        if (next - value).abs() > 1.0e-6 {
            changed = Some(next);
        }
    }
    if ui.pointer_in(track).is_some()
        && ui.input().wheel().y.abs() > 0.0
        && !ui.input().modifiers().control
    {
        let step = (maximum - minimum) * 0.03;
        changed = Some((value + ui.input().wheel().y.signum() * step).clamp(minimum, maximum));
    }
    let shown = changed.unwrap_or(value);
    let target = normalize(shown);
    let position = ui.spring_to(key, target, Motion::quick());
    let grow = ui.animate(
        key ^ 0x51,
        if response.hovered || response.active {
            1.0
        } else {
            0.0
        },
        Motion::quick(),
    );
    let accent = ui.theme().palette.accent;
    let track_color = Color::white_alpha(0.13 + 0.05 * grow);
    let outline = Border {
        width: 0.5,
        color: Color::white_alpha(0.05),
    };
    let bar = Rect::from_center_size(
        Vec2::new(track.center().x, rect.center().y),
        Vec2::new(track.width(), 4.0 + 1.4 * grow),
    );
    let knob_center = Vec2::new(bar.min.x + bar.width() * position, bar.center().y);
    let radius = 6.0 + 1.2 * grow;
    let knob = Rect::from_center_size(knob_center, Vec2::splat(radius * 2.0));
    let filled = Rect::from_min_max(
        bar.min,
        Vec2::new(
            (knob_center.x - radius * 0.2).max(bar.min.x + bar.height() * 0.5),
            bar.max.y,
        ),
    );
    let readout_style = TextStyle::new(
        ui.theme().font(Level::Monospace),
        ui.theme().palette.text_secondary,
    )
    .aligned(Align::End);
    let mut painter = ui.painter();
    if let Some(text) = label {
        painter.text_at(Vec2::new(rect.min.x, rect.center().y - 7.0), text, caption);
    }
    painter.outlined(
        bar,
        Corners::capsule(),
        Fill::Solid(track_color),
        outline,
        Shadow::NONE,
    );
    painter.rect(filled, Corners::capsule(), accent.with_alpha(0.92));
    painter.shadow(knob, Corners::capsule(), knob_shadow());
    painter.rect(knob, Corners::capsule(), Color::WHITE);
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
            ui.set_cursor(crate::registry::Cursor::ResizeHorizontal);
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
        rect.shrink2(Vec2::new(7.0, 0.0)),
        &text,
        style,
        Align::Center,
    );
    changed
}

pub fn segmented(ui: &mut Ui, id: &str, options: &[&str], selected: usize) -> Option<usize> {
    let key = ui.id(id);
    let metrics = ui.theme().metrics;
    let font = ui.theme().font(Level::Callout);
    let style = TextStyle::new(font, Color::WHITE);
    let widths = options
        .iter()
        .map(|option| ui.measure_text(option, style).x + 22.0)
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
        if ui.interact(cell_key, cell).clicked {
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
    let hovered = ui.input().position();
    let accent_text = ui.theme().palette.text_primary;
    let quiet_text = ui.theme().palette.text_secondary;
    let selected_look = ui
        .theme()
        .look(Role::SegmentSelected, crate::style::State::Rest);
    let mut painter = ui.painter();
    painter.outlined(
        rect,
        Corners::capsule(),
        Fill::Solid(Color::rgba(0.0, 0.0, 0.0, 0.22)),
        Border {
            width: metrics.hairline,
            color: Color::white_alpha(0.06),
        },
        Shadow::NONE,
    );
    if !response.active {
        for (index, cell) in cells.iter().enumerate() {
            if cell.contains(hovered) && index != target {
                painter.rect(*cell, Corners::capsule(), Color::white_alpha(0.06));
            }
        }
    }

    painter.outlined(
        pill,
        Corners::capsule(),
        selected_look.fill,
        selected_look.border,
        selected_look.shadow,
    );
    for (index, cell) in cells.iter().enumerate() {
        let color = if index == target {
            accent_text
        } else {
            quiet_text
        };
        let style = TextStyle::new(font, color).aligned(Align::Center);
        painter.text_in(*cell, options[index], style, Align::Center);
    }
    changed
}

pub fn tabs(ui: &mut Ui, id: &str, options: &[&str], selected: usize) -> Option<usize> {
    segmented(ui, id, options, selected)
}

pub fn disclosure(
    ui: &mut Ui,
    id: &str,
    title: &str,
    open: bool,
    content: impl FnOnce(&mut Ui),
) -> bool {
    let key = ui.id(id);
    let header = ui.allocate_full(28.0);
    let response = ui.interact(key, header);
    let next = if response.clicked { !open } else { open };
    let arrow = ui.animate(key, if open { 1.0 } else { 0.0 }, Motion::snappy());
    let font = ui.theme().font(Level::Headline);
    let color = ui.theme().text(Level::Headline);
    let chevron_color = ui.theme().palette.text_tertiary;
    let style = TextStyle::new(font, color);
    let hover = ui.animate(
        key ^ 0x31,
        if response.hovered { 1.0 } else { 0.0 },
        Motion::quick(),
    );
    {
        let mut painter = ui.painter();
        if hover > 0.01 {
            painter.rect(
                header.shrink(2.0),
                Corners::all(7.0),
                Color::white_alpha(0.05 * hover),
            );
        }
        painter.text_at(
            Vec2::new(header.min.x + 16.0, header.center().y - 8.0),
            title,
            style,
        );
        let centre = Vec2::new(header.min.x + 6.0, header.center().y);
        let angle = std::f32::consts::FRAC_PI_2 * (0.5 + 0.5 * arrow);
        let tip = centre + Vec2::new(angle.cos(), angle.sin()) * 2.4;
        painter.segment(tip + Vec2::new(-2.1, -1.3), tip, 1.4, chevron_color);
        painter.segment(tip, tip + Vec2::new(2.1, -1.3), 1.4, chevron_color);
    }
    if next {
        ui.column(Block::column().gaps(ui.theme().metrics.gap), content);
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
        size * 0.38,
        1.6,
        0.0,
        std::f32::consts::TAU,
        color.with_alpha(0.16),
    );
    painter.ring(center, size * 0.38, 1.6, time * 5.0, 1.7, color);
}

pub fn progress(ui: &mut Ui, fraction: f32, width: f32) {
    let rect = ui.allocate(Vec2::new(width, 4.0));
    let accent = ui.theme().palette.accent;
    let mut painter = ui.painter();
    painter.rect(rect, Corners::capsule(), Color::white_alpha(0.12));
    let width = rect.width() * fraction.clamp(0.0, 1.0);
    if width > 1.0 {
        painter.rect(
            Rect::from_min_size(rect.min, Vec2::new(width, rect.height())),
            Corners::capsule(),
            accent,
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
            width: 0.5,
            color: Color::white_alpha(if response.hovered { 0.6 } else { 0.25 }),
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
    let metrics = ui.theme().metrics;
    let name = TextStyle::new(
        ui.theme().font(Level::Caption),
        ui.theme().palette.text_tertiary,
    );
    let content = TextStyle::new(
        ui.theme().font(Level::Monospace),
        ui.theme().text(Level::Callout),
    );
    let width = ui.available().x;
    let label_width = (width * 0.42).clamp(64.0, 128.0);
    let rect = ui.allocate(Vec2::new(width, 18.0));
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
    let _ = metrics;
}

pub fn section_title(ui: &mut Ui, title: &str) {
    let style = TextStyle::new(
        ui.theme().font(Level::Micro),
        ui.theme().palette.text_tertiary,
    );
    let rect = ui.allocate_full(20.0);
    let upper = title.to_uppercase();
    let mut painter = ui.painter();
    painter.text_at(Vec2::new(rect.min.x, rect.center().y - 7.0), &upper, style);
}

pub fn caption(ui: &mut Ui, text: &str) {
    let style = TextStyle::new(
        ui.theme().font(Level::Caption),
        ui.theme().text(Level::Caption),
    );
    ui.text_style(text, style);
}

pub fn weak(ui: &mut Ui, text: &str) {
    let style = TextStyle::new(
        ui.theme().font(Level::Caption),
        ui.theme().palette.text_tertiary,
    );
    let available = ui.available().x;
    ui.text_style_wrapped(text, style, available);
}

pub fn body(ui: &mut Ui, text: &str) {
    let style = TextStyle::new(ui.theme().font(Level::Body), ui.theme().text(Level::Body));
    ui.text_style(text, style);
}

pub fn body_wrapped(ui: &mut Ui, text: &str) {
    let style = TextStyle::new(ui.theme().font(Level::Body), ui.theme().text(Level::Body));
    let available = ui.available().x;
    ui.text_style_wrapped(text, style.wrapping(), available);
}

pub fn headline(ui: &mut Ui, text: &str) {
    let style = TextStyle::new(
        ui.theme().font(Level::Headline),
        ui.theme().text(Level::Headline),
    );
    ui.text_style(text, style);
}

pub fn title(ui: &mut Ui, text: &str) {
    let style = TextStyle::new(ui.theme().font(Level::Title), ui.theme().text(Level::Title));
    ui.text_style(text, style);
}

pub fn field_label(ui: &mut Ui, text: &str) {
    let style = TextStyle::new(
        ui.theme().font(Level::Micro),
        ui.theme().palette.text_tertiary,
    );
    ui.text_style(text, style);
}

pub fn row(ui: &mut Ui, gap: f32, content: impl FnOnce(&mut Ui)) -> Rect {
    ui.row(Block::row().gaps(gap).align(Align::Center), content)
}

pub fn column(ui: &mut Ui, gap: f32, content: impl FnOnce(&mut Ui)) -> Rect {
    ui.column(Block::column().gaps(gap), content)
}
