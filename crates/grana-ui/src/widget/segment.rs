use glam::Vec2;

use crate::color::Color;
use crate::geometry::{Corners, Rect};
use crate::glass::GlassStyle;
use crate::input::Button;
use crate::motion::Motion;
use crate::text::{Align, TextStyle};
use crate::theme::Level;
use crate::ui::Ui;

pub fn segmented(ui: &mut Ui, id: &str, options: &[&str], selected: usize) -> Option<usize> {
    segmented_core(ui, id, options, selected, false)
}

pub fn segmented_fill(ui: &mut Ui, id: &str, options: &[&str], selected: usize) -> Option<usize> {
    segmented_core(ui, id, options, selected, true)
}

fn segmented_core(
    ui: &mut Ui,
    id: &str,
    options: &[&str],
    selected: usize,
    fill: bool,
) -> Option<usize> {
    let key = ui.id(id);
    let metrics = ui.theme().metrics;
    let palette = ui.theme().palette;
    let font = ui.theme().font(Level::Callout).medium();
    let style = TextStyle::new(font, Color::WHITE);
    let measured = options
        .iter()
        .map(|option| ui.measure_text(option, style).x + 24.0)
        .collect::<Vec<_>>();
    let total = if fill {
        ui.available().x
    } else {
        measured.iter().sum::<f32>() + 6.0
    };
    let widths = if fill && !options.is_empty() {
        vec![(total - 6.0) / options.len() as f32; options.len()]
    } else {
        measured
    };
    let rect = ui.allocate(Vec2::new(total, metrics.control_height));
    let mut changed = None;
    let mut dragging = false;
    let mut engaged = false;
    let mut left = rect.min.x + 3.0;
    let mut cells = Vec::new();
    for (index, width) in widths.iter().enumerate() {
        let cell = Rect::from_min_size(
            Vec2::new(left, rect.min.y + 3.0),
            Vec2::new(*width, rect.height() - 6.0),
        );
        cells.push(cell);
        let cell_key = key ^ (index as u64 + 1).wrapping_mul(0x9e37_79b9_7f4a_7c15);
        let response = ui.interact(cell_key, cell);
        dragging |= response.dragging;
        engaged |= response.active;
        let corners = Corners::capsule();
        if ui.focusable(cell_key, cell, corners) && ui.activated(cell_key) {
            changed = Some(index);
        }
        if response.clicked {
            changed = Some(index);
        }
        left += width;
    }
    let pointer = ui.input().position();
    if dragging
        && ui.input().button(Button::Primary).down
        && let Some(index) = cells.iter().position(|cell| cell.contains(pointer))
    {
        changed = Some(index);
    }
    let target = changed
        .unwrap_or(selected)
        .min(options.len().saturating_sub(1));
    let target_cell = cells.get(target).copied().unwrap_or(rect);
    let dragging = dragging && ui.input().button(Button::Primary).down && rect.contains(pointer);
    let width_target = target_cell.width();
    let x_target = if dragging {
        (pointer.x - width_target * 0.5).clamp(rect.min.x + 3.0, rect.max.x - 3.0 - width_target)
    } else {
        target_cell.min.x
    };
    let motion = if dragging {
        Motion::response(0.16, 0.12)
    } else {
        Motion::response(0.42, 0.22)
    };
    let x = ui.spring_to(key, x_target, motion);
    let width = ui.spring_to(key ^ 0x1f, width_target, motion);
    let pill = Rect::from_min_size(
        Vec2::new(x, rect.min.y + 3.0),
        Vec2::new(width, rect.height() - 6.0),
    );
    let mut hovered = Vec::with_capacity(cells.len());
    for (index, cell) in cells.iter().enumerate() {
        hovered.push(ui.animate(
            key ^ (index as u64 + 7).wrapping_mul(0x517c_c1b7_2722_0a95),
            if index != target && cell.contains(pointer) {
                1.0
            } else {
                0.0
            },
            Motion::quick(),
        ));
    }
    {
        let mut painter = ui.painter();
        painter.rect(
            rect,
            Corners::capsule(),
            palette.material_fill.amplify(0.75),
        );
        for (index, cell) in cells.iter().enumerate() {
            let hover = hovered[index];
            if hover > 0.01 {
                painter.rect(
                    *cell,
                    Corners::capsule(),
                    palette.material_hover.amplify(0.5 * hover),
                );
            }
        }
        for (index, cell) in cells.iter().enumerate() {
            let active = index == target;
            let color = if active {
                palette.text_primary
            } else {
                palette
                    .text_secondary
                    .lerp(palette.text_primary, hovered[index])
            };
            let style = TextStyle::new(font, color).aligned(Align::Center);
            painter.text_in(*cell, options[index], style, Align::Center);
        }
    }
    let pill_key = key ^ 0x2f;
    ui.glass(pill_key, pill, GlassStyle::tab_lens());
    ui.engage_glass(pill_key, engaged);
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
    let key = ui.id(id);
    let palette = ui.theme().palette;
    let metrics = ui.theme().metrics;
    let font = ui.theme().font(Level::Callout).medium();
    let style = TextStyle::new(font, Color::WHITE);
    let height = 32.0;
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
    let mut dragging = false;
    let mut engaged = false;
    let mut left = rect.min.x;
    let mut cells = Vec::new();
    for (index, width) in widths.iter().enumerate() {
        let cell = Rect::from_min_size(
            Vec2::new(left, rect.min.y + 3.0),
            Vec2::new(*width, height - 6.0),
        );
        cells.push(cell);
        let cell_key = ui.id(&format!("{id}.tab.{index}"));
        let response = ui.interact(cell_key, cell);
        dragging |= response.dragging;
        engaged |= response.active;
        if ui.focusable(cell_key, cell, Corners::capsule()) && ui.activated(cell_key) {
            changed = Some(index);
        }
        if response.clicked {
            changed = Some(index);
        }
        left += width;
    }
    let pointer = ui.input().position();
    if dragging
        && ui.input().button(Button::Primary).down
        && let Some(index) = cells.iter().position(|cell| cell.contains(pointer))
    {
        changed = Some(index);
    }
    let target = changed
        .unwrap_or(selected)
        .min(options.len().saturating_sub(1));
    let target_cell = cells.get(target).copied().unwrap_or(rect);
    let total_tabs_width = left - rect.min.x;
    let dragging = dragging && ui.input().button(Button::Primary).down && rect.contains(pointer);
    let width_target = target_cell.width();
    let x_target = if dragging {
        (pointer.x - width_target * 0.5).clamp(
            rect.min.x,
            (rect.min.x + total_tabs_width - width_target).max(rect.min.x),
        )
    } else {
        target_cell.min.x
    };
    let motion = if dragging {
        Motion::response(0.16, 0.12)
    } else {
        Motion::response(0.42, 0.20)
    };
    let x = ui.spring_to(key, x_target, motion);
    let width = ui.spring_to(key ^ 0x2f, width_target, motion);
    let pill = Rect::from_min_size(
        Vec2::new(x, rect.min.y + 3.0),
        Vec2::new(width, height - 6.0),
    );
    let mut hovered = Vec::with_capacity(cells.len());
    for (index, cell) in cells.iter().enumerate() {
        hovered.push(ui.animate(
            key ^ (index as u64 + 3).wrapping_mul(0x9e37_79b9_7f4a_7c15),
            if cell.contains(pointer) && index != target {
                1.0
            } else {
                0.0
            },
            Motion::quick(),
        ));
    }
    {
        let mut painter = ui.painter();
        for (index, cell) in cells.iter().enumerate() {
            let hover = hovered[index];
            if hover > 0.01 {
                painter.rect(
                    cell.shrink2(Vec2::new(2.0, 0.0)),
                    Corners::capsule(),
                    palette.material_hover.amplify(0.4 * hover),
                );
            }
        }
        for (index, cell) in cells.iter().enumerate() {
            let active = index == target;
            let color = if active {
                palette.text_primary
            } else {
                palette
                    .text_secondary
                    .lerp(palette.text_primary, hovered[index])
            };
            painter.text_in(
                *cell,
                options[index],
                TextStyle::new(font, color).aligned(Align::Center),
                Align::Center,
            );
        }
    }
    let pill_key = key ^ 0x71;
    ui.glass(pill_key, pill, GlassStyle::tab_lens());
    ui.engage_glass(pill_key, engaged);
    changed
}
