use glam::Vec2;
use grana_measure::Enrichment;
use grana_ui::{Align, Corners, Painter, Rect, TextStyle};

use crate::layout::cell_size;
use crate::style::Style;

const GAP: f32 = 1.5;

const LABEL_WIDTH: f32 = 38.0;

const LABEL_LIMIT: usize = 12;

pub fn paint(
    painter: &mut Painter,
    rect: Rect,
    enrichment: &Enrichment,
    style: &Style,
    empty: Option<&str>,
) {
    let metrics = style.metrics;
    let palette = style.palette;
    let frame = metrics.frame(rect);
    let count = enrichment.classes().len();
    if count == 0 {
        if let Some(empty) = empty {
            painter.text_in(
                frame,
                empty,
                crate::distribution::empty_style(style),
                Align::Center,
            );
        }
        return;
    }
    let size = cell_size(frame, count, count);
    let gap = GAP.min(size.x.min(size.y) * 0.15);
    let ceiling = enrichment.largest().clamp(2.0, 6.0);
    for row in 0..count {
        for column in 0..count {
            let pair = enrichment.pair(row, column);
            let weight = (pair.z().abs() / ceiling).clamp(0.0, 1.0) as f32;
            let tint = if pair.z() >= 0.0 {
                palette.error
            } else {
                palette.primary
            };
            let color = palette.surface_container_low.lerp(tint, weight);
            let left = frame.min.x + size.x * column as f32 + 0.5 * gap;
            let right = frame.min.x + size.x * (column + 1) as f32 - 0.5 * gap;
            let top = frame.max.y - size.y * (row + 1) as f32 + 0.5 * gap;
            let bottom = frame.max.y - size.y * row as f32 - 0.5 * gap;
            painter.rect(
                Rect::from_min_max(
                    Vec2::new(left, top),
                    Vec2::new(right.max(left + 0.5), bottom.max(top + 0.5)),
                ),
                Corners::ZERO,
                color,
            );
        }
    }
    let step = count.div_ceil(LABEL_LIMIT).max(1);
    let label = TextStyle::new(metrics.caption, palette.on_surface_variant);
    let height = (size.y * step as f32).max(14.0);
    let left = Rect::from_min_max(
        Vec2::new(frame.min.x - LABEL_WIDTH, frame.min.y),
        Vec2::new(frame.min.x - 4.0, frame.max.y),
    );
    for row in (0..count).step_by(step) {
        let centre = frame.max.y - size.y * (row as f32 + 0.5);
        painter.text_in(
            Rect::from_min_max(
                Vec2::new(left.min.x, centre - 0.5 * height),
                Vec2::new(left.max.x, centre + 0.5 * height),
            ),
            &enrichment.classes()[row].code().to_string(),
            label.aligned(Align::End),
            Align::Center,
        );
    }
    let width = size.x * step as f32;
    let bottom = Rect::from_min_max(
        Vec2::new(frame.min.x, frame.max.y + 2.0),
        Vec2::new(frame.max.x, frame.max.y + metrics.tick_height),
    );
    for column in (0..count).step_by(step) {
        let centre = frame.min.x + size.x * (column as f32 + 0.5);
        painter.text_in(
            Rect::from_min_max(
                Vec2::new(centre - 0.5 * width, bottom.min.y),
                Vec2::new(centre + 0.5 * width, bottom.max.y),
            ),
            &enrichment.classes()[column].code().to_string(),
            label.aligned(Align::Center),
            Align::Center,
        );
    }
}
