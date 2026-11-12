use glam::Vec2;
use grana_measure::{Distribution, DistributionSeries};
use grana_ui::{Align, Border, Color, Corners, Fill, Painter, Rect, Shadow, TextStyle};

use crate::axis::Axis;
use crate::decor::{BrushKind, Decor, Highlight};
use crate::grid;
use crate::layout::{group_at, group_centre, slot as slot_width};
use crate::style::{DistributionStyle, Style};

const GROUP_LABEL: f32 = 58.0;

#[derive(Clone, Copy)]
pub(crate) struct Slot {
    pub(crate) centre: f32,
    pub(crate) width: f32,
    pub(crate) frame: Rect,
}

pub fn paint(
    painter: &mut Painter,
    rect: Rect,
    distribution: &Distribution,
    style: &Style,
    decor: &Decor<'_>,
    empty: Option<&str>,
) {
    let metrics = style.metrics;
    let palette = style.palette;
    let accent = palette.primary;
    let frame = metrics.frame(rect);
    let groups = distribution.groups();
    if groups.is_empty() {
        if let Some(empty) = empty {
            painter.text_in(frame, empty, empty_style(style), Align::Center);
        }
        return;
    }
    let y = Axis::new(distribution.low(), distribution.high(), 4);
    grid::axis(painter, rect, style, y);
    grid::slot_separators(painter, frame, style, groups.len());
    let tick = metrics.tick_band(rect);
    let hovered = decor
        .pointer
        .and_then(|pointer| group_at(frame, groups.len(), pointer));
    let step =
        ((groups.len() as f32 * GROUP_LABEL / frame.width().max(1.0)).ceil() as usize).max(1);
    let width = (0.46 * slot_width(frame, groups.len())).clamp(6.0, 34.0);
    let gating = distribution.gated() < distribution.total();
    let cold = TextStyle::new(metrics.caption, palette.outline)
        .aligned(Align::Center)
        .truncating();
    let hot = TextStyle::new(metrics.caption, palette.on_surface)
        .aligned(Align::Center)
        .truncating();
    for (index, group) in groups.iter().enumerate() {
        let centre = group_centre(frame, index, groups.len());
        let slot = Slot {
            centre,
            width,
            frame,
        };
        if hovered == Some(index) {
            let half = 0.5 * slot_width(frame, groups.len());
            painter.rect(
                Rect::from_min_max(
                    Vec2::new(centre - half, frame.min.y),
                    Vec2::new(centre + half, frame.max.y),
                ),
                Corners::ZERO,
                palette.on_surface.with_alpha(0.035),
            );
        }
        if gating {
            shape(painter, style, group.total(), y, slot, false);
            if let Some(series) = group.gated() {
                shape(painter, style, series, y, slot, true);
            }
        } else {
            shape(painter, style, group.total(), y, slot, true);
        }
        if index % step == 0 || hovered == Some(index) {
            let reach = (slot_width(frame, groups.len()) * step as f32).max(28.0);
            let label = grana_i18n::measure::distribution_group(style.language, group.label());
            painter.text_in(
                Rect::from_center_size(
                    Vec2::new(centre, tick.center().y),
                    Vec2::new(reach, metrics.tick_height),
                ),
                &label,
                if hovered == Some(index) { hot } else { cold },
                Align::Center,
            );
        }
    }
    if let Some((low, high)) = decor.gates.x {
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
    if let Some(Highlight::Group { index, value }) = decor.highlight
        && index < groups.len()
    {
        let centre = Vec2::new(group_centre(frame, index, groups.len()), y.y(frame, value));
        painter.outlined(
            Rect::from_center_size(centre, Vec2::splat(9.0)),
            Corners::capsule(),
            Fill::Solid(palette.on_surface.with_alpha(0.10)),
            Border {
                width: 1.3,
                color: accent,
            },
            Shadow::NONE,
        );
    }
}

pub(crate) fn shape(
    painter: &mut Painter,
    style: &Style,
    series: &DistributionSeries,
    y: Axis,
    slot: Slot,
    filled: bool,
) {
    let accent = style.palette.primary;
    match style.distribution {
        DistributionStyle::Box => box_shape(painter, style, series, y, slot, accent, filled),
        DistributionStyle::Violin => violin_shape(painter, series, y, slot, accent, filled),
    }
}

fn box_shape(
    painter: &mut Painter,
    style: &Style,
    series: &DistributionSeries,
    y: Axis,
    slot: Slot,
    accent: Color,
    filled: bool,
) {
    let Slot {
        centre,
        width,
        frame,
    } = slot;
    let alpha = if filled { 1.0 } else { 0.45 };
    let border = accent.with_alpha(0.72 * alpha);
    let whisker_high = y.y(frame, series.whisker_high() as f32);
    let whisker_low = y.y(frame, series.whisker_low() as f32);
    painter.segment(
        Vec2::new(centre, whisker_high),
        Vec2::new(centre, whisker_low),
        1.0,
        border,
    );
    let cap = 0.32 * width;
    for pixel in [whisker_high, whisker_low] {
        painter.segment(
            Vec2::new(centre - cap, pixel),
            Vec2::new(centre + cap, pixel),
            1.0,
            border,
        );
    }
    let upper = y.y(frame, series.upper_quartile() as f32);
    let lower = y.y(frame, series.lower_quartile() as f32);
    let top = upper.min(lower);
    let bottom = upper.max(lower).max(top + 1.5);
    painter.outlined(
        Rect::from_min_max(
            Vec2::new(centre - 0.5 * width, top),
            Vec2::new(centre + 0.5 * width, bottom),
        ),
        Corners::all(2.0),
        if filled {
            Fill::Solid(accent.with_alpha(0.20))
        } else {
            Fill::None
        },
        Border {
            width: 1.0,
            color: border,
        },
        Shadow::NONE,
    );
    let median = y.y(frame, series.median() as f32);
    painter.segment(
        Vec2::new(centre - 0.5 * width, median),
        Vec2::new(centre + 0.5 * width, median),
        if filled { 1.8 } else { 1.0 },
        accent.with_alpha(if filled { 0.95 } else { 0.45 }),
    );
    let mean = y.y(frame, series.mean() as f32);
    painter.rect(
        Rect::from_center_size(Vec2::new(centre, mean), Vec2::splat(3.2)),
        Corners::capsule(),
        style
            .palette
            .on_surface
            .with_alpha(if filled { 0.78 } else { 0.32 }),
    );
    for (index, value) in series.outliers().iter().enumerate() {
        let pixel = y.y(frame, *value);
        let spread = ((index as f32 * 37.0) % 23.0) / 22.0 - 0.5;
        painter.rect(
            Rect::from_center_size(
                Vec2::new(centre + spread * 0.5 * width, pixel),
                Vec2::splat(2.0),
            ),
            Corners::capsule(),
            accent.with_alpha(if filled { 0.5 } else { 0.22 }),
        );
    }
}

fn violin_shape(
    painter: &mut Painter,
    series: &DistributionSeries,
    y: Axis,
    slot: Slot,
    accent: Color,
    filled: bool,
) {
    let Slot {
        centre,
        width,
        frame,
    } = slot;
    let density = series.density();
    if density.is_empty() {
        return;
    }
    let points = density.len();
    let reach = 0.5 * width;
    let fill = accent.with_alpha(if filled { 0.26 } else { 0.10 });
    let edge = accent.with_alpha(if filled { 0.62 } else { 0.32 });
    let rows = frame.height().ceil().max(1.0) as usize;
    let sample = |position: f32| {
        let index = position.clamp(0.0, 1.0) * (points - 1) as f32;
        let low = index.floor() as usize;
        let high = (low + 1).min(points - 1);
        let weight = index - low as f32;
        density[low] * (1.0 - weight) + density[high] * weight
    };
    if filled {
        if painter.recording() {
            let half = |index: usize| sample(index as f32 / (points - 1) as f32).clamp(0.0, 1.0);
            let row =
                |index: usize| frame.max.y - (index as f32 + 0.5) / points as f32 * frame.height();
            let mut outline = Vec::with_capacity(points * 2);
            for index in 0..points {
                outline.push(Vec2::new(centre + half(index) * reach, row(index)));
            }
            for index in (0..points).rev() {
                outline.push(Vec2::new(centre - half(index) * reach, row(index)));
            }
            painter.contour(&outline, fill);
        } else {
            for row in 0..rows {
                let t = (row as f32 + 0.5) / rows as f32;
                let half = sample(1.0 - t).clamp(0.0, 1.0) * reach;
                if half <= 0.15 {
                    continue;
                }
                let top = frame.min.y + row as f32;
                painter.rect(
                    Rect::from_min_max(
                        Vec2::new(centre - half, top),
                        Vec2::new(centre + half, top + 1.0),
                    ),
                    Corners::ZERO,
                    fill,
                );
            }
        }
    }
    let mut previous: Option<(f32, f32, f32)> = None;
    for (index, density) in density.iter().enumerate() {
        let pixel = frame.max.y - (index as f32 + 0.5) / points as f32 * frame.height();
        let half = density.clamp(0.0, 1.0) * reach;
        let left = centre - half;
        let right = centre + half;
        if let Some((last, last_left, last_right)) = previous {
            painter.segment(
                Vec2::new(last_left, last),
                Vec2::new(left, pixel),
                1.0,
                edge,
            );
            painter.segment(
                Vec2::new(last_right, last),
                Vec2::new(right, pixel),
                1.0,
                edge,
            );
        }
        previous = Some((pixel, left, right));
    }
    painter.segment(
        Vec2::new(centre, frame.min.y),
        Vec2::new(centre, frame.max.y),
        0.5,
        edge.with_alpha(0.5),
    );
    let median = y.y(frame, series.median() as f32);
    painter.segment(
        Vec2::new(centre - 0.5 * width, median),
        Vec2::new(centre + 0.5 * width, median),
        1.4,
        accent.with_alpha(if filled { 0.85 } else { 0.42 }),
    );
}

pub(crate) fn empty_style(style: &Style) -> TextStyle {
    TextStyle::new(style.metrics.caption, style.palette.outline).aligned(Align::Center)
}
