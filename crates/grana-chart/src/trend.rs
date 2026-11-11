use glam::Vec2;
use grana_measure::{Trend, TrendPoint};
use grana_ui::{Align, Border, Corners, Fill, Painter, Rect, Shadow};

use crate::axis::Axis;
use crate::decor::{BrushKind, Decor, Highlight};
use crate::distribution::empty_style;
use crate::grid;
use crate::style::Style;

pub fn paint(
    painter: &mut Painter,
    rect: Rect,
    trend: &Trend,
    style: &Style,
    decor: &Decor<'_>,
    empty: Option<&str>,
) {
    let palette = style.palette;
    let accent = palette.accent;
    let frame = style.metrics.frame(rect);
    let frames = trend.frames();
    let x = Axis::frames(1.0, frames.max(2) as f32, 5);
    let y = Axis::new(trend.low(), trend.high(), 4);
    grid::grid(painter, rect, style, x, y);
    if trend.points().is_empty() {
        if let Some(empty) = empty {
            painter.text_in(frame, empty, empty_style(style), Align::Center);
        }
        return;
    }
    if let Some((low, high)) = decor.gates.y {
        let one = y.y(frame, low);
        let two = y.y(frame, high);
        painter.rect(
            Rect::from_min_max(
                Vec2::new(frame.min.x, one.min(two)),
                Vec2::new(frame.max.x, one.max(two)),
            ),
            Corners::ZERO,
            accent.with_alpha(0.05),
        );
        for pixel in [one, two] {
            painter.segment(
                Vec2::new(frame.min.x, pixel),
                Vec2::new(frame.max.x, pixel),
                0.8,
                accent.with_alpha(0.32),
            );
        }
    }
    if let Some(brush) = decor
        .brush
        .filter(|brush| brush.kind == BrushKind::Rectangle)
    {
        let from = frame.max.y - brush.from.y.clamp(0.0, 1.0) * frame.height();
        let to = frame.max.y - brush.to.y.clamp(0.0, 1.0) * frame.height();
        painter.outlined(
            Rect::from_min_max(
                Vec2::new(frame.min.x, from.min(to)),
                Vec2::new(frame.max.x, from.max(to)),
            ),
            Corners::ZERO,
            Fill::Solid(accent.with_alpha(0.08)),
            Border {
                width: 0.75,
                color: accent.with_alpha(0.52),
            },
            Shadow::NONE,
        );
    }
    let points = trend
        .points()
        .iter()
        .map(|point| Projected::of(point, x, y, frame))
        .collect::<Vec<_>>();
    for pair in points.windows(2) {
        if pair[1].time != pair[0].time + 1 {
            continue;
        }
        let band = accent.with_alpha(0.16);
        painter.triangle(
            Vec2::new(pair[0].pixel, pair[0].upper),
            Vec2::new(pair[1].pixel, pair[1].upper),
            Vec2::new(pair[1].pixel, pair[1].lower),
            band,
        );
        painter.triangle(
            Vec2::new(pair[0].pixel, pair[0].upper),
            Vec2::new(pair[1].pixel, pair[1].lower),
            Vec2::new(pair[0].pixel, pair[0].lower),
            band,
        );
    }
    for point in &points {
        if point.upper - point.lower > 0.5 {
            painter.segment(
                Vec2::new(point.pixel, point.upper),
                Vec2::new(point.pixel, point.lower),
                1.0,
                accent.with_alpha(0.28),
            );
        }
        painter.rect(
            Rect::from_center_size(Vec2::new(point.pixel, point.mean), Vec2::splat(2.6)),
            Corners::capsule(),
            accent.with_alpha(0.9),
        );
    }
    for pair in points.windows(2) {
        if pair[1].time != pair[0].time + 1 {
            continue;
        }
        painter.segment(
            Vec2::new(pair[0].pixel, pair[0].mean),
            Vec2::new(pair[1].pixel, pair[1].mean),
            1.8,
            accent,
        );
    }
    if let Some(Highlight::Point(point)) = decor.highlight {
        let centre = Vec2::new(x.x(frame, point.x), y.y(frame, point.y));
        painter.outlined(
            Rect::from_center_size(centre, Vec2::splat(9.0)),
            Corners::capsule(),
            Fill::Solid(palette.text_primary.with_alpha(0.10)),
            Border {
                width: 1.3,
                color: accent,
            },
            Shadow::NONE,
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
}

struct Projected {
    time: u32,
    pixel: f32,
    upper: f32,
    lower: f32,
    mean: f32,
}

impl Projected {
    fn of(point: &TrendPoint, x: Axis, y: Axis, frame: Rect) -> Self {
        Self {
            time: point.time(),
            pixel: x.x(frame, point.time() as f32 + 1.0),
            upper: y.y(frame, point.upper()),
            lower: y.y(frame, point.lower()),
            mean: y.y(frame, point.mean()),
        }
    }
}
