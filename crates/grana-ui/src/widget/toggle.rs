use glam::Vec2;

use crate::color::Color;
use crate::geometry::{Corners, Rect};
use crate::motion::Motion;
use crate::style::{Border, Fill, Shadow};
use crate::text::{Align, TextStyle};
use crate::theme::Level;
use crate::ui::Ui;

use super::knob_shadow;

pub fn toggle(ui: &mut Ui, id: &str, value: bool) -> Option<bool> {
    let key = ui.id(id);
    let metrics = ui.theme().metrics;
    let rect = ui.allocate(Vec2::new(metrics.switch_width, metrics.switch_height));
    let activated = ui.focusable(key, rect, Corners::capsule()) && ui.activated(key);
    switch(ui, key, rect, value).or(activated.then_some(!value))
}

fn switch(ui: &mut Ui, key: u64, rect: Rect, value: bool) -> Option<bool> {
    let palette = ui.theme().palette;
    let response = ui.interact(key, rect);
    let changed = response.clicked.then_some(!value);
    let target = if value { 1.0 } else { 0.0 };
    let progress = ui.spring_to(key, target, Motion::response(0.36, 0.22));
    let engaged = ui.animate(
        key ^ 0x11,
        if response.hovered || response.active {
            1.0
        } else {
            0.0
        },
        Motion::quick(),
    );
    let press = ui.animate(
        key ^ 0x12,
        if response.active { 1.0 } else { 0.0 },
        Motion::quick(),
    );
    let track = Rect::from_min_max(
        Vec2::new(rect.min.x, rect.center().y - rect.height() * 0.5 + 1.0),
        Vec2::new(rect.max.x, rect.center().y + rect.height() * 0.5 - 1.0),
    );
    let knob = rect.height() + engaged * 0.9 + press * 0.9;
    let travel = (rect.width() - knob).max(0.0);
    let position = progress.clamp(-0.04, 1.04);
    let current = rect.min.x + travel * position;
    let target_x = rect.min.x + travel * target;
    let lag = target_x - current;
    let reach = (lag.abs() * 0.6).min(knob * 0.5);
    let (left, right) = if lag >= 0.0 {
        (current, current + knob + reach)
    } else {
        (current - reach, current + knob)
    };
    let knob_rect = Rect::from_min_max(
        Vec2::new(left, rect.center().y - knob * 0.5),
        Vec2::new(right, rect.center().y + knob * 0.5),
    );
    let fill = palette
        .track
        .lerp(palette.success, progress.clamp(0.0, 1.0));
    let mut painter = ui.painter();
    painter.outlined(
        track,
        Corners::capsule(),
        Fill::Solid(fill),
        Border {
            width: 1.0,
            color: palette.shade.amplify(0.55),
        },
        Shadow::NONE,
    );
    let inner = track.shrink(1.0);
    let clip = painter.clip();
    painter.push_clip(inner, Corners::capsule());
    painter.gradient(
        Rect::from_min_size(inner.min, Vec2::new(inner.width(), inner.height() * 0.5)),
        Corners::ZERO,
        palette.shade.amplify(0.35),
        Color::TRANSPARENT,
    );
    painter.pop_clip(clip);
    painter.shadow(knob_rect, Corners::capsule(), knob_shadow());
    painter.gradient(
        knob_rect,
        Corners::capsule(),
        palette.knob,
        palette.knob_shade,
    );
    let highlight = Rect::from_min_max(
        Vec2::new(
            knob_rect.min.x + knob_rect.width() * 0.28,
            knob_rect.min.y + 1.5,
        ),
        Vec2::new(
            knob_rect.max.x - knob_rect.width() * 0.28,
            knob_rect.min.y + 2.6,
        ),
    );
    painter.rect(highlight, Corners::capsule(), Color::white_alpha(0.75));
    changed
}

pub fn toggle_row(ui: &mut Ui, id: &str, label: &str, value: bool) -> Option<bool> {
    let key = ui.id(id);
    let metrics = ui.theme().metrics;
    let palette = ui.theme().palette;
    let rect = ui.allocate(Vec2::new(ui.available().x, metrics.row_height));
    let mut response = ui.interact(key, rect);
    if ui.focusable(key, rect, Corners::all(9.0)) && ui.activated(key) {
        response.clicked = true;
    }
    let changed = response.clicked.then_some(!value);
    let hover = ui.animate(
        key ^ 0x21,
        if response.hovered { 1.0 } else { 0.0 },
        Motion::quick(),
    );
    let font = ui.theme().font(Level::Callout).medium();
    let switch_rect = Rect::from_center_size(
        Vec2::new(rect.max.x - metrics.switch_width * 0.5, rect.center().y),
        Vec2::new(metrics.switch_width, metrics.switch_height),
    );
    {
        let mut painter = ui.painter();
        if hover > 0.01 {
            painter.rect(
                rect.shrink2(Vec2::new(1.0, 0.5)),
                Corners::all(9.0),
                palette.material_hover.amplify(0.55 * hover),
            );
        }
        painter.text_in(
            Rect::from_min_max(rect.min, Vec2::new(switch_rect.min.x - 10.0, rect.max.y)),
            label,
            TextStyle::new(
                font,
                palette.text_secondary.lerp(
                    palette.text_primary,
                    hover.max(if value { 1.0 } else { 0.0 }),
                ),
            )
            .truncating(),
            Align::Center,
        );
    }
    switch(ui, key ^ 0x2f, switch_rect, value).or(changed)
}

pub fn checkbox(ui: &mut Ui, id: &str, label: &str, value: bool) -> Option<bool> {
    let key = ui.id(id);
    let palette = ui.theme().palette;
    let font = ui.theme().font(Level::Callout);
    let measured = ui.measure_text(label, TextStyle::new(font, Color::WHITE));
    let rect = ui.allocate(Vec2::new(measured.x + 28.0, 26.0));
    let mut response = ui.interact(key, rect);
    if ui.focusable(key, rect, Corners::capsule()) && ui.activated(key) {
        response.clicked = true;
    }
    let changed = response.clicked.then_some(!value);
    let mark = ui.animate(
        key ^ 0x31,
        if value { 1.0 } else { 0.0 },
        Motion::response(0.28, 0.28),
    );
    let hover = ui.animate(
        key ^ 0x32,
        if response.hovered { 1.0 } else { 0.0 },
        Motion::quick(),
    );
    let box_rect = Rect::from_center_size(
        Vec2::new(rect.min.x + 9.0, rect.center().y),
        Vec2::splat(17.0),
    );
    let outline = palette.text_tertiary.lerp(palette.text_secondary, hover);
    let fill = palette.accent.with_alpha(mark);
    let text_color = palette
        .text_secondary
        .lerp(palette.text_primary, hover.max(mark));
    let mut painter = ui.painter();
    painter.outlined(
        box_rect,
        Corners::all(5.0),
        Fill::Solid(fill),
        Border {
            width: 1.2,
            color: outline.lerp(palette.accent, mark),
        },
        Shadow::NONE,
    );
    if mark > 0.02 {
        let color = palette.text_on_accent.with_alpha(mark);
        let left = Vec2::new(box_rect.min.x + 4.2, box_rect.min.y + 8.6);
        let bottom = Vec2::new(box_rect.min.x + 7.2, box_rect.max.y - 4.4);
        let right = Vec2::new(box_rect.max.x - 3.6, box_rect.min.y + 4.8);
        painter.segment(left, bottom, 1.9, color);
        painter.segment(bottom, right, 1.9, color);
    }
    painter.text_in(
        Rect::from_min_max(Vec2::new(rect.min.x + 24.0, rect.min.y), rect.max),
        label,
        TextStyle::new(font, text_color),
        Align::Center,
    );
    changed
}
