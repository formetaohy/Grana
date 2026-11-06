use std::ops::RangeInclusive;

use glam::Vec2;

use crate::color::Color;
use crate::geometry::{Corners, Rect};
use crate::glass::GlassStyle;
use crate::input::Key;
use crate::motion::Motion;
use crate::registry::Cursor;
use crate::style::{Border, Fill, Shadow};
use crate::text::{Align, TextStyle};
use crate::theme::{Level, Role};
use crate::ui::{Length, Ui};

pub struct SliderText<'a> {
    pub label: Option<&'a str>,
    pub readout: Option<&'a str>,
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

pub fn slider_compact(
    ui: &mut Ui,
    id: &str,
    value: f32,
    range: RangeInclusive<f32>,
) -> Option<f32> {
    slider(ui, id, None, value, range, false)
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

fn slider_core(
    ui: &mut Ui,
    id: &str,
    value: f32,
    range: RangeInclusive<f32>,
    logarithmic: bool,
    width: Length,
    text: SliderText<'_>,
) -> Option<f32> {
    let key = ui.id(id);
    let metrics = ui.theme().metrics;
    let palette = ui.theme().palette;
    let caption = TextStyle::new(ui.theme().label_font(), palette.text_tertiary);
    let label_width = text.label.map_or(0.0, |label| {
        ui.measure_text(label, caption).x.min(96.0) + 12.0
    });
    let rect = ui.allocate_sized(width, metrics.control_height);
    let readout_width = if text.readout.is_some() { 56.0 } else { 0.0 };
    let hit = Rect::from_min_max(
        Vec2::new(rect.min.x + label_width, rect.center().y - 13.0),
        Vec2::new(rect.max.x - readout_width, rect.center().y + 13.0),
    );
    let bar = Rect::from_center_size(hit.center(), Vec2::new(hit.width(), metrics.track));
    let knob = metrics.thumb + 4.0;
    let knob_height = 17.0;
    let travel = (bar.width() - knob).max(0.0);
    let response = ui.interact(key, hit);
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
        let next = denormalize((pointer - (bar.min.x + knob * 0.5)) / travel.max(1.0));
        if (next - value).abs() > 1.0e-6 {
            changed = Some(next);
        }
        ui.set_cursor(Cursor::ResizeHorizontal);
    }
    if response.hovered && ui.input().wheel().y.abs() > 0.0 && !ui.input().modifiers().control {
        let step = (maximum - minimum) * 0.03;
        changed = Some((value + ui.input().wheel().y.signum() * step).clamp(minimum, maximum));
    }
    if ui.focusable(key, hit, Corners::all(metrics.radius)) {
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
    let position = ui.spring_to(
        key,
        target,
        if response.active {
            Motion::response(0.18, 0.18)
        } else {
            Motion::response(0.32, 0.2)
        },
    );
    let engaged = ui.animate(
        key ^ 0x51,
        if response.hovered || response.active {
            1.0
        } else {
            0.0
        },
        Motion::quick(),
    );
    let size_w = knob + engaged * 2.0;
    let size_h = knob_height + engaged * 1.5;
    let position = position.clamp(0.0, 1.0);
    let center = Vec2::new(bar.min.x + knob * 0.5 + travel * position, bar.center().y);
    let lag = (target - position) * travel;
    let reach = (lag.abs() * 0.48).min(knob * 0.45);
    let (left, right) = if lag >= 0.0 {
        (center.x - size_w * 0.5, center.x + size_w * 0.5 + reach)
    } else {
        (center.x - size_w * 0.5 - reach, center.x + size_w * 0.5)
    };
    let knob_rect = Rect::from_min_max(
        Vec2::new(left, center.y - size_h * 0.5),
        Vec2::new(right, center.y + size_h * 0.5),
    );
    let fill_end = (knob_rect.min.x + position * knob_rect.width()).clamp(bar.min.x, bar.max.x);
    let filled = Rect::from_min_max(bar.min, Vec2::new(fill_end, bar.max.y));
    let readout_style = TextStyle::new(ui.theme().font(Level::Monospace), palette.text_secondary)
        .aligned(Align::End);
    {
        let mut painter = ui.painter();
        if let Some(label) = text.label {
            painter.text_in(
                Rect::from_min_max(
                    Vec2::new(rect.min.x, rect.min.y),
                    Vec2::new(rect.min.x + label_width - 8.0, rect.max.y),
                ),
                label,
                TextStyle::new(caption.font, caption.color).truncating(),
                Align::Center,
            );
        }
        painter.rect(
            bar,
            Corners::capsule(),
            palette
                .track
                .with_alpha(palette.track.alpha * (0.85 + 0.3 * engaged)),
        );
        if filled.width() > 0.0 {
            painter.gradient(
                filled,
                Corners::capsule(),
                palette.accent_hover,
                palette.accent,
            );
        }
        if let Some(readout) = text.readout {
            painter.text_in(
                Rect::from_min_max(
                    Vec2::new(rect.max.x - readout_width + 6.0, rect.min.y),
                    rect.max,
                ),
                readout,
                readout_style,
                Align::Center,
            );
        }
    }
    let knob_key = key ^ 0x5d;
    ui.glass(knob_key, knob_rect, GlassStyle::knob());
    ui.engage_glass(knob_key, response.active);
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
    let rect = ui.allocate_full(32.0);
    let (minimum, maximum) = (*range.start(), *range.end());
    let span = (maximum - minimum).max(1.0e-6);
    let bar = Rect::from_min_max(
        Vec2::new(rect.min.x + 8.0, rect.center().y - 4.0),
        Vec2::new(rect.max.x - 8.0, rect.center().y + 4.0),
    );
    let normalize = |value: f32| ((value - minimum) / span).clamp(0.0, 1.0);
    let denormalize = |value: f32| minimum + value.clamp(0.0, 1.0) * span;
    let low_key = key ^ 0x21;
    let high_key = key ^ 0x43;
    let low_x = bar.min.x + bar.width() * normalize(start);
    let high_x = bar.min.x + bar.width() * normalize(end);
    let low_rect = Rect::from_center_size(Vec2::new(low_x, bar.center().y), Vec2::new(30.0, 30.0));
    let high_rect =
        Rect::from_center_size(Vec2::new(high_x, bar.center().y), Vec2::new(30.0, 30.0));
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
    painter_track(ui, bar, palette.track);
    if gradient.len() > 1 {
        let clip = ui.painter().clip();
        {
            let mut painter = ui.painter();
            painter.push_clip(bar, Corners::capsule());
            let segments = gradient.len() - 1;
            for (index, color) in gradient.iter().take(segments).enumerate() {
                let left = bar.min.x + bar.width() * index as f32 / segments as f32;
                let right = bar.min.x + bar.width() * (index + 1) as f32 / segments as f32;
                painter.rect(
                    Rect::from_min_max(
                        Vec2::new(left, bar.min.y),
                        Vec2::new(right + 0.6, bar.max.y),
                    ),
                    Corners::ZERO,
                    *color,
                );
            }
            painter.pop_clip(clip);
        }
    }
    let low_t = ui.spring_to(
        key ^ 0x31,
        normalize(next_start),
        if low.active {
            Motion::response(0.16, 0.12)
        } else {
            Motion::response(0.32, 0.2)
        },
    );
    let high_t = ui.spring_to(
        key ^ 0x33,
        normalize(next_end),
        if high.active {
            Motion::response(0.16, 0.12)
        } else {
            Motion::response(0.32, 0.2)
        },
    );
    let low_x = bar.min.x + bar.width() * low_t.clamp(0.0, 1.0);
    let high_x = bar.min.x + bar.width() * high_t.clamp(0.0, 1.0);
    let shade = palette.shade.amplify(0.55);
    let knob_rect = |x: f32, active: bool| {
        Rect::from_center_size(
            Vec2::new(x, bar.center().y),
            Vec2::new(
                12.0 + grow * 2.0,
                20.0 + grow * 3.0 + if active { 2.0 } else { 0.0 },
            ),
        )
    };
    let low_knob = knob_rect(low_x, low.active);
    let high_knob = knob_rect(high_x, high.active);
    {
        let mut painter = ui.painter();
        painter.rect(
            Rect::from_min_max(bar.min, Vec2::new(low_x, bar.max.y)),
            Corners {
                top_left: 4.0,
                bottom_left: 4.0,
                ..Corners::ZERO
            },
            shade,
        );
        painter.rect(
            Rect::from_min_max(Vec2::new(high_x, bar.min.y), bar.max),
            Corners {
                top_right: 4.0,
                bottom_right: 4.0,
                ..Corners::ZERO
            },
            shade,
        );
    }
    for (index, (knob, active)) in [(low_knob, low.active), (high_knob, high.active)]
        .into_iter()
        .enumerate()
    {
        let knob_key = key ^ (0x61 + index as u64);
        ui.glass(knob_key, knob, GlassStyle::knob());
        ui.engage_glass(knob_key, active);
    }
    let changed = (next_start - start).abs() > 1.0e-6 || (next_end - end).abs() > 1.0e-6;
    changed.then_some((
        next_start.clamp(minimum, maximum),
        next_end.clamp(minimum, maximum),
    ))
}

fn painter_track(ui: &mut Ui, bar: Rect, color: Color) {
    let mut painter = ui.painter();
    painter.outlined(
        bar,
        Corners::capsule(),
        Fill::Solid(color),
        Border::NONE,
        Shadow::NONE,
    );
}

pub fn drag_value(ui: &mut Ui, id: &str, value: f32, speed: f32, decimals: usize) -> Option<f32> {
    let key = ui.id(id);
    let metrics = ui.theme().metrics;
    let rect = ui.allocate(Vec2::new(66.0, metrics.control_height));
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
    } else if response.hovered {
        ui.set_cursor(Cursor::ResizeHorizontal);
        if ui.input().wheel().y.abs() > 0.0 {
            changed = Some(value + ui.input().wheel().y * speed * 8.0);
        }
    }
    let look = ui.surface(key, Role::Field, &response);
    let font = ui.theme().font(Level::Monospace);
    let style = TextStyle::new(font, look.text).aligned(Align::End);
    let text = format!("{:.*}", decimals, changed.unwrap_or(value));
    let mut painter = ui.painter();
    painter.outlined(rect, look.corners, look.fill, look.border, look.shadow);
    painter.text_in(
        rect.shrink2(Vec2::new(9.0, 0.0)),
        &text,
        style,
        Align::Center,
    );
    changed
}

pub fn progress(ui: &mut Ui, fraction: f32, width: f32) {
    let palette = ui.theme().palette;
    let rect = ui.allocate(Vec2::new(width, 4.0));
    let mut painter = ui.painter();
    painter.rect(rect, Corners::capsule(), palette.track);
    let filled = rect.width() * fraction.clamp(0.0, 1.0);
    if filled > 1.0 {
        painter.gradient(
            Rect::from_min_size(rect.min, Vec2::new(filled, rect.height())),
            Corners::capsule(),
            palette.accent_hover,
            palette.accent,
        );
    }
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
    let clip = ui.painter().clip();
    let mut painter = ui.painter();
    painter.push_clip(rect, Corners::capsule());
    let segments = colors.len() - 1;
    for (index, color) in colors.iter().take(segments).enumerate() {
        let left = rect.min.x + rect.width() * index as f32 / segments as f32;
        let right = rect.min.x + rect.width() * (index + 1) as f32 / segments as f32;
        painter.rect(
            Rect::from_min_max(
                Vec2::new(left, rect.min.y),
                Vec2::new(right + 0.6, rect.max.y),
            ),
            Corners::ZERO,
            *color,
        );
    }
    painter.pop_clip(clip);
}
