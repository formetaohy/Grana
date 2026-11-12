use glam::Vec2;
use grana_ui::{Align, Painter, Rect, TextStyle};

use crate::axis::Axis;
use crate::layout::slot;
use crate::style::{Metrics, Style};

pub(crate) fn grid(painter: &mut Painter, rect: Rect, style: &Style, x: Axis, y: Axis) {
    let metrics = style.metrics;
    let frame = metrics.frame(rect);
    let tick = metrics.tick_band(rect);
    let line = style.palette.outline_variant.with_alpha(0.35);
    let text = TextStyle::new(metrics.tick, style.palette.outline);
    for value in x.ticks() {
        let pixel = x.x(frame, value);
        painter.segment(
            Vec2::new(pixel, frame.min.y),
            Vec2::new(pixel, frame.max.y),
            0.5,
            line,
        );
        painter.text_in(
            tick_box(metrics, tick, pixel),
            &x.label(value),
            text.aligned(Align::Center),
            Align::Center,
        );
    }
    axis(painter, rect, style, y);
}

pub(crate) fn axis(painter: &mut Painter, rect: Rect, style: &Style, y: Axis) {
    axis_in(painter, rect, style.metrics.frame(rect), style, y);
}

pub(crate) fn axis_in(painter: &mut Painter, rect: Rect, frame: Rect, style: &Style, y: Axis) {
    let metrics = style.metrics;
    let text = TextStyle::new(metrics.tick, style.palette.outline);
    let line = style.palette.outline_variant.with_alpha(0.35);
    for value in y.ticks() {
        let pixel = y.y(frame, value);
        painter.segment(
            Vec2::new(frame.min.x, pixel),
            Vec2::new(frame.max.x, pixel),
            0.5,
            line,
        );
        painter.text_in(
            Rect::from_min_max(
                Vec2::new(rect.min.x + 4.0, pixel - 0.5 * metrics.tick_height),
                Vec2::new(frame.min.x - 6.0, pixel + 0.5 * metrics.tick_height),
            ),
            &y.label(value),
            text.aligned(Align::End),
            Align::Center,
        );
    }
}

pub(crate) fn tick_box(metrics: Metrics, band: Rect, pixel: f32) -> Rect {
    let half = 0.5 * metrics.tick_width;
    let low = band.min.x + half;
    let high = (band.max.x - half).max(low);
    Rect::from_center_size(
        Vec2::new(pixel.clamp(low, high), band.center().y),
        Vec2::new(metrics.tick_width, metrics.tick_height),
    )
}

pub(crate) fn slot_separators(painter: &mut Painter, frame: Rect, style: &Style, count: usize) {
    let line = style.palette.outline_variant.with_alpha(0.28);
    for index in 1..count {
        let x = frame.min.x + slot(frame, count) * index as f32;
        painter.segment(
            Vec2::new(x, frame.min.y),
            Vec2::new(x, frame.max.y),
            0.5,
            line,
        );
    }
}
