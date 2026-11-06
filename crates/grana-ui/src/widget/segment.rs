use glam::Vec2;

use crate::color::Color;
use crate::geometry::{Corners, Rect};
use crate::glass::GlassStyle;
use crate::motion::Motion;
use crate::style::Shadow;
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
        let corners = Corners::capsule();
        if ui.focusable(cell_key, cell, corners) && ui.activated(cell_key) {
            changed = Some(index);
        }
        if response.clicked {
            changed = Some(index);
        }
        left += width;
    }
    let target = selected.min(options.len().saturating_sub(1));
    let target_cell = cells.get(target).copied().unwrap_or(rect);
    let x = ui.spring_to(key, target_cell.min.x, Motion::response(0.4, 0.16));
    let width = ui.spring_to(key ^ 0x1f, target_cell.width(), Motion::response(0.4, 0.16));
    let pill = Rect::from_min_size(
        Vec2::new(x, rect.min.y + 3.0),
        Vec2::new(width, rect.height() - 6.0),
    );
    let pointer = ui.input().position();
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
    painter.shadow(
        pill,
        Corners::capsule(),
        Shadow::new(Color::black_alpha(0.28), 8.0, 2.0),
    );
    painter.gradient(
        pill,
        Corners::capsule(),
        palette.selected.amplify(1.0),
        palette.selected.amplify(0.72),
    );
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
        if ui.focusable(cell_key, cell, Corners::capsule()) && ui.activated(cell_key) {
            changed = Some(index);
        }
        if response.clicked {
            changed = Some(index);
        }
        left += width;
    }
    let target = selected.min(options.len().saturating_sub(1));
    let target_cell = cells.get(target).copied().unwrap_or(rect);
    let x = ui.spring_to(key, target_cell.min.x, Motion::response(0.42, 0.14));
    let width = ui.spring_to(
        key ^ 0x2f,
        target_cell.width(),
        Motion::response(0.42, 0.14),
    );
    let pill = Rect::from_min_size(
        Vec2::new(x, rect.min.y + 3.0),
        Vec2::new(width, height - 6.0),
    );
    let pointer = ui.input().position();
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
    }
    ui.glass(key ^ 0x71, pill, GlassStyle::tab_lens().interactive());
    let mut painter = ui.labels_painter();
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
    changed
}
