use glam::Vec2;
use grana_measure::Scatter;
use grana_ui::{Align, Border, Corners, Fill, Painter, Rect, Shadow};

use crate::axis::Axis;
use crate::decor::{BrushKind, Decor, Highlight};
use crate::distribution::empty_style;
use crate::grid;
use crate::layout::{cell_centre, cell_size};
use crate::style::Style;

pub fn paint(
    painter: &mut Painter,
    rect: Rect,
    scatter: &Scatter,
    style: &Style,
    decor: &Decor<'_>,
    empty: Option<&str>,
) {
    let palette = style.palette;
    let accent = palette.primary;
    let frame = style.metrics.frame(rect);
    let x = Axis::new(scatter.x_low(), scatter.x_high(), 5);
    let y = Axis::new(scatter.y_low(), scatter.y_high(), 4);
    grid::grid(painter, rect, style, x, y);
    match (decor.gates.x, decor.gates.y) {
        (Some(first), Some(second)) => {
            let bounds = Rect::from_min_max(
                Vec2::new(x.x(frame, first.0), y.y(frame, second.1)),
                Vec2::new(x.x(frame, first.1), y.y(frame, second.0)),
            );
            painter.outlined(
                bounds,
                Corners::ZERO,
                Fill::Solid(accent.with_alpha(0.05)),
                Border {
                    width: 0.75,
                    color: accent.with_alpha(0.32),
                },
                Shadow::NONE,
            );
        }
        (Some(gate), None) => {
            let low = x.x(frame, gate.0);
            let high = x.x(frame, gate.1);
            painter.rect(
                Rect::from_min_max(Vec2::new(low, frame.min.y), Vec2::new(high, frame.max.y)),
                Corners::ZERO,
                accent.with_alpha(0.05),
            );
        }
        (None, Some(gate)) => {
            let low = y.y(frame, gate.1);
            let high = y.y(frame, gate.0);
            painter.rect(
                Rect::from_min_max(Vec2::new(frame.min.x, low), Vec2::new(frame.max.x, high)),
                Corners::ZERO,
                accent.with_alpha(0.05),
            );
        }
        (None, None) => {}
    }
    let scale = (1.0 + scatter.peak().max(1) as f32).ln();
    let x_bins = scatter.x_bins();
    let y_bins = scatter.y_bins();
    let cell = cell_size(frame, x_bins, y_bins);
    let dimmed = decor.lasso.is_some();
    let ink = |alpha: f32| {
        palette
            .on_surface
            .with_alpha(if dimmed { alpha * 0.5 } else { alpha })
    };
    if decor.sparse {
        for row in 0..y_bins {
            for column in 0..x_bins {
                if scatter.total_at(column, row) == 0 {
                    continue;
                }
                let centre = cell_centre(frame, column, row, x_bins, y_bins);
                let dot = cell.x.min(cell.y).clamp(1.8, 3.0);
                painter.rect(
                    Rect::from_center_size(centre, Vec2::splat(dot)),
                    Corners::capsule(),
                    ink(0.5),
                );
            }
        }
        for row in 0..y_bins {
            for column in 0..x_bins {
                if scatter.gated_at(column, row) == 0 {
                    continue;
                }
                let centre = cell_centre(frame, column, row, x_bins, y_bins);
                let dot = cell.x.min(cell.y).clamp(2.2, 3.4);
                painter.rect(
                    Rect::from_center_size(centre, Vec2::splat(dot)),
                    Corners::capsule(),
                    accent.with_alpha(0.9),
                );
            }
        }
    } else {
        for row in 0..y_bins {
            for column in 0..x_bins {
                let total = scatter.total_at(column, row);
                if total == 0 {
                    continue;
                }
                let centre = cell_centre(frame, column, row, x_bins, y_bins);
                let weight = ((1.0 + total as f32).ln() / scale).clamp(0.0, 1.0);
                painter.rect(
                    Rect::from_center_size(centre, Vec2::new(cell.x + 0.5, cell.y + 0.5)),
                    Corners::ZERO,
                    ink(0.04 + 0.34 * weight),
                );
                let gated = scatter.gated_at(column, row);
                if gated > 0 {
                    let weight = ((1.0 + gated as f32).ln() / scale).clamp(0.0, 1.0);
                    painter.rect(
                        Rect::from_center_size(centre, Vec2::new(cell.x * 0.7, cell.y * 0.7)),
                        Corners::ZERO,
                        accent.with_alpha(0.22 + 0.68 * weight),
                    );
                }
            }
        }
    }
    if let Some(lasso) = decor.lasso {
        outline(painter, frame, x, y, lasso.points(), accent);
    }
    if let Some(brush) = decor.brush {
        let reach = |point: Vec2| {
            Vec2::new(
                frame.min.x + point.x.clamp(0.0, 1.0) * frame.width(),
                frame.max.y - point.y.clamp(0.0, 1.0) * frame.height(),
            )
        };
        match brush.kind {
            BrushKind::Rectangle => {
                let from = reach(brush.from);
                let to = reach(brush.to);
                painter.outlined(
                    Rect::from_min_max(from.min(to), from.max(to)),
                    Corners::ZERO,
                    Fill::Solid(accent.with_alpha(0.10)),
                    Border {
                        width: 0.75,
                        color: accent.with_alpha(0.55),
                    },
                    Shadow::NONE,
                );
            }
            BrushKind::Lasso => {
                let mut pixels = brush.points.iter().copied().map(reach).collect::<Vec<_>>();
                pixels.push(reach(brush.to));
                trace(painter, &pixels, accent);
            }
        }
    }
    if let Some(Highlight::Point(point)) = decor.highlight {
        let centre = Vec2::new(x.x(frame, point.x), y.y(frame, point.y));
        painter.outlined(
            Rect::from_center_size(centre, Vec2::splat(9.0)),
            Corners::capsule(),
            Fill::Solid(palette.on_surface.with_alpha(0.12)),
            Border {
                width: 1.4,
                color: accent,
            },
            Shadow::NONE,
        );
    }
    if let Some(pointer) = decor.pointer {
        let line = palette.on_surface.with_alpha(0.10);
        painter.segment(
            Vec2::new(pointer.x, frame.min.y),
            Vec2::new(pointer.x, frame.max.y),
            0.6,
            line,
        );
        painter.segment(
            Vec2::new(frame.min.x, pointer.y),
            Vec2::new(frame.max.x, pointer.y),
            0.6,
            line,
        );
    }
    if scatter.peak() == 0
        && let Some(empty) = empty
    {
        painter.text_in(frame, empty, empty_style(style), Align::Center);
    }
}

fn outline(
    painter: &mut Painter,
    frame: Rect,
    x: Axis,
    y: Axis,
    points: &[[f32; 2]],
    accent: grana_ui::Color,
) {
    if points.len() < 2 {
        return;
    }
    let pixel = |point: [f32; 2]| Vec2::new(x.x(frame, point[0]), y.y(frame, point[1]));
    let mut previous = pixel(points[points.len() - 1]);
    for point in points {
        let current = pixel(*point);
        painter.segment(previous, current, 4.0, accent.with_alpha(0.10));
        painter.segment(previous, current, 1.0, accent.with_alpha(0.75));
        previous = current;
    }
}

fn trace(painter: &mut Painter, points: &[Vec2], accent: grana_ui::Color) {
    if points.len() < 2 {
        return;
    }
    let mut previous = points[points.len() - 1];
    for point in points {
        painter.segment(previous, *point, 4.0, accent.with_alpha(0.10));
        painter.segment(previous, *point, 1.0, accent.with_alpha(0.75));
        previous = *point;
    }
}
