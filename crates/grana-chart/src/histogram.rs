use glam::Vec2;
use grana_measure::Histogram;
use grana_ui::{Corners, Painter, Rect};

use crate::axis::Axis;
use crate::decor::{Brush, BrushKind, Decor, Highlight};
use crate::distribution::empty_style;
use crate::grid;
use crate::style::Style;

pub fn paint(
    painter: &mut Painter,
    rect: Rect,
    histogram: &Histogram,
    style: &Style,
    decor: &Decor<'_>,
) {
    let metrics = style.metrics;
    let palette = style.palette;
    let accent = palette.accent;
    let frame = metrics.frame(rect);
    let x = Axis::new(histogram.low(), histogram.high(), 5);
    let y = Axis::new(0.0, histogram.peak().max(1) as f32, 4);
    grid::grid(painter, rect, style, x, y);
    let bins = histogram.bins();
    let width = frame.width() / bins.max(1) as f32;
    let gap = if width > 4.0 { 1.0 } else { 0.0 };
    for bin in 0..bins {
        let total = histogram.total(bin);
        if total == 0 {
            continue;
        }
        let left = frame.min.x + width * bin as f32;
        let bar = (width - gap).max(0.5);
        let height = total as f32 / y.high() * frame.height();
        painter.rect(
            Rect::from_min_max(
                Vec2::new(left, frame.max.y - height),
                Vec2::new(left + bar, frame.max.y),
            ),
            Corners::top(1.5),
            palette.text_primary.with_alpha(0.14),
        );
        let gated = histogram.gated(bin);
        if gated > 0 {
            let height = gated as f32 / y.high() * frame.height();
            painter.rect(
                Rect::from_min_max(
                    Vec2::new(left, frame.max.y - height),
                    Vec2::new(left + bar, frame.max.y),
                ),
                Corners::top(1.5),
                accent.with_alpha(0.85),
            );
        }
    }
    if let Some((low, high)) = decor.gates.x {
        let one = x.x(frame, low);
        let two = x.x(frame, high);
        painter.rect(
            Rect::from_min_max(
                Vec2::new(one.min(two), frame.min.y),
                Vec2::new(one.max(two), frame.max.y),
            ),
            Corners::ZERO,
            accent.with_alpha(0.07),
        );
        for pixel in [one, two] {
            painter.segment(
                Vec2::new(pixel, frame.min.y),
                Vec2::new(pixel, frame.max.y),
                0.8,
                accent.with_alpha(0.35),
            );
        }
    }
    if let Some(brush) = decor
        .brush
        .filter(|brush| brush.kind == BrushKind::Rectangle)
    {
        let band = brush_band(frame, brush);
        painter.rect(
            Rect::from_min_max(
                Vec2::new(band.0, frame.min.y),
                Vec2::new(band.1, frame.max.y),
            ),
            Corners::ZERO,
            accent.with_alpha(0.06),
        );
        for pixel in [band.0, band.1] {
            painter.segment(
                Vec2::new(pixel, frame.min.y),
                Vec2::new(pixel, frame.max.y),
                0.8,
                accent.with_alpha(0.52),
            );
        }
    }
    if let Some(Highlight::Value(value)) = decor.highlight {
        let pixel = x.x(frame, value);
        painter.segment(
            Vec2::new(pixel, frame.min.y),
            Vec2::new(pixel, frame.max.y),
            1.4,
            accent,
        );
        painter.rect(
            Rect::from_center_size(Vec2::new(pixel, frame.max.y - 4.0), Vec2::splat(5.0)),
            Corners::capsule(),
            accent,
        );
    }
    if let Some(pointer) = decor.pointer {
        painter.segment(
            Vec2::new(pointer.x, frame.min.y),
            Vec2::new(pointer.x, frame.max.y),
            0.6,
            palette.text_primary.with_alpha(0.12),
        );
    }
    if histogram.peak() == 0 {
        painter.text_in(
            frame,
            "No objects to plot",
            empty_style(style),
            grana_ui::Align::Center,
        );
    }
}

fn brush_band(frame: Rect, brush: Brush<'_>) -> (f32, f32) {
    let one = frame.min.x + brush.from.x.clamp(0.0, 1.0) * frame.width();
    let two = frame.min.x + brush.to.x.clamp(0.0, 1.0) * frame.width();
    (one.min(two), one.max(two))
}
