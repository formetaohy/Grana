use glam::Vec2;
use grana_measure::{Ripley, Trend};
use grana_ui::{Align, Border, Color, Corners, Fill, Painter, Rect, Shadow, TextStyle, Theme};

use crate::axis::Axis;
use crate::chart::{HEIGHT, Heading, WIDTH};
use crate::decor::{BrushKind, Decor, Highlight};
use crate::distribution::empty_style;
use crate::grid;
use crate::style::{DistributionStyle, Style};

const LEGEND_HEIGHT: f32 = 13.0;
const LEGEND_GAP: f32 = 14.0;
const LEGEND_INSET: f32 = 6.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CurvePoint {
    pub x: f32,
    pub value: f32,
    pub lower: f32,
    pub upper: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Curve {
    pub label: String,
    pub points: Vec<CurvePoint>,
}

pub struct SeriesChart {
    pub x: Axis,
    pub y: Axis,
    pub curves: Vec<Curve>,
}

impl SeriesChart {
    pub fn of_trend(trend: &Trend) -> Self {
        let frames = trend.frames().max(2) as f32;
        Self {
            x: Axis::frames(1.0, frames, 5),
            y: Axis::new(trend.low(), trend.high(), 4),
            curves: vec![Curve {
                label: String::new(),
                points: trend
                    .points()
                    .iter()
                    .map(|point| CurvePoint {
                        x: point.time() as f32 + 1.0,
                        value: point.value(),
                        lower: point.lower(),
                        upper: point.upper(),
                    })
                    .collect(),
            }],
        }
    }

    pub fn of_ripley(ripley: &Ripley) -> Self {
        Self {
            x: Axis::new(0.0, ripley.radius(), 5),
            y: Axis::new(ripley.low(), ripley.high(), 4),
            curves: vec![Curve {
                label: String::new(),
                points: ripley
                    .points()
                    .iter()
                    .map(|point| CurvePoint {
                        x: point.radius(),
                        value: point.value(),
                        lower: point.lower(),
                        upper: point.upper(),
                    })
                    .collect(),
            }],
        }
    }

    pub fn empty(&self) -> bool {
        self.curves.iter().all(|curve| curve.points.is_empty())
    }
}

pub struct SeriesPage {
    pub heading: Heading,
    pub chart: SeriesChart,
    pub style: DistributionStyle,
}

impl SeriesPage {
    pub fn size(&self) -> [u32; 2] {
        [WIDTH, HEIGHT]
    }

    pub fn paint(&self, painter: &mut Painter, rect: Rect, style: &Style, decor: &Decor<'_>) {
        let style = Style {
            language: self.heading.language,
            ..*style
        };
        crate::chart::heading(painter, rect, &self.heading, &style);
        paint(
            painter,
            rect,
            &self.chart,
            &style,
            decor,
            self.heading.empty.as_deref(),
        );
    }

    pub(crate) fn marks(&self) -> Vec<grana_ui::Mark> {
        let mut recorder = grana_ui::Recorder::new(Theme::dark());
        let size = self.size();
        let points = Vec2::new(size[0] as f32, size[1] as f32);
        let page = Rect::from_min_size(Vec2::ZERO, points);
        let theme = *recorder.theme();
        let style = Style::page(&theme, self.style);
        let heading = &self.heading;
        let chart = &self.chart;
        recorder.marks(points, |painter| {
            painter.rect(page, Corners::ZERO, theme.palette.scene_backdrop);
            let style = Style {
                language: heading.language,
                ..style
            };
            crate::chart::heading(painter, page, heading, &style);
            paint(
                painter,
                page,
                chart,
                &style,
                &Decor::default(),
                heading.empty.as_deref(),
            );
        })
    }
}

pub fn paint(
    painter: &mut Painter,
    rect: Rect,
    chart: &SeriesChart,
    style: &Style,
    decor: &Decor<'_>,
    empty: Option<&str>,
) {
    let metrics = style.metrics;
    let palette = style.palette;
    let accent = palette.accent;
    let frame = metrics.frame(rect);
    grid::grid(painter, rect, style, chart.x, chart.y);
    if chart.empty() {
        if let Some(empty) = empty {
            painter.text_in(frame, empty, empty_style(style), Align::Center);
        }
        return;
    }
    if let Some((low, high)) = decor.gates.y {
        let one = chart.y.y(frame, low);
        let two = chart.y.y(frame, high);
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
    let single = chart.curves.len() == 1;
    let projected = chart
        .curves
        .iter()
        .enumerate()
        .map(|(index, curve)| {
            (
                tint(style, index),
                curve
                    .points
                    .iter()
                    .map(|point| {
                        (
                            chart.x.x(frame, point.x),
                            chart.y.y(frame, point.value),
                            chart.y.y(frame, point.lower),
                            chart.y.y(frame, point.upper),
                            point.upper - point.lower,
                        )
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<Vec<_>>();
    for (color, points) in &projected {
        for pair in points.windows(2) {
            let band = color.with_alpha(0.13);
            painter.triangle(
                Vec2::new(pair[0].0, pair[0].2),
                Vec2::new(pair[1].0, pair[1].2),
                Vec2::new(pair[1].0, pair[1].3),
                band,
            );
            painter.triangle(
                Vec2::new(pair[0].0, pair[0].2),
                Vec2::new(pair[1].0, pair[1].3),
                Vec2::new(pair[0].0, pair[0].3),
                band,
            );
        }
    }
    for (color, points) in &projected {
        if single {
            for point in points {
                if point.4 > 0.5 {
                    painter.segment(
                        Vec2::new(point.0, point.2),
                        Vec2::new(point.0, point.3),
                        1.0,
                        color.with_alpha(0.30),
                    );
                }
            }
        }
        for pair in points.windows(2) {
            painter.segment(
                Vec2::new(pair[0].0, pair[0].1),
                Vec2::new(pair[1].0, pair[1].1),
                if single { 1.8 } else { 1.6 },
                *color,
            );
        }
        if single || points.len() < 2 {
            for point in points {
                painter.rect(
                    Rect::from_center_size(Vec2::new(point.0, point.1), Vec2::splat(2.6)),
                    Corners::capsule(),
                    color.with_alpha(0.9),
                );
            }
        }
    }
    legend(painter, frame, style, &chart.curves);
    if let Some(Highlight::Point(point)) = decor.highlight {
        let centre = Vec2::new(chart.x.x(frame, point.x), chart.y.y(frame, point.y));
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

fn legend(painter: &mut Painter, frame: Rect, style: &Style, curves: &[Curve]) {
    let metrics = style.metrics;
    let text = TextStyle::new(metrics.caption, style.palette.text_tertiary);
    let mut entries = Vec::new();
    let mut pen = LEGEND_INSET;
    let mut row = LEGEND_INSET;
    let row_height = LEGEND_HEIGHT + 4.0;
    let mut width = 0.0f32;
    for (index, curve) in curves.iter().enumerate() {
        if curve.label.is_empty() {
            continue;
        }
        let measured = painter.measure(&curve.label, text).x;
        if pen > LEGEND_INSET && pen + 16.0 + measured > frame.width() - 2.0 * LEGEND_INSET {
            pen = LEGEND_INSET;
            row += row_height;
        }
        if row + LEGEND_HEIGHT > frame.height() - LEGEND_INSET {
            break;
        }
        entries.push((index, curve.label.as_str(), pen, row, measured));
        pen += 16.0 + measured + LEGEND_GAP;
        width = width.max(pen - LEGEND_GAP + LEGEND_INSET);
    }
    if entries.len() <= 1 && curves.len() <= 1 {
        return;
    }
    if entries.is_empty() {
        return;
    }
    let height = row + LEGEND_HEIGHT + LEGEND_INSET;
    painter.rect(
        Rect::from_min_size(frame.min + Vec2::splat(2.0), Vec2::new(width, height)),
        Corners::all(6.0),
        style.palette.scene_backdrop.with_alpha(0.72),
    );
    for (index, label, x, y, _) in entries {
        let centre = frame.min + Vec2::new(x + 2.0 + 6.0, y + 2.0 + 0.5 * LEGEND_HEIGHT);
        painter.rect(
            Rect::from_center_size(centre, Vec2::new(12.0, 3.0)),
            Corners::capsule(),
            tint(style, index),
        );
        painter.text_at(frame.min + Vec2::new(x + 2.0 + 16.0, y + 2.0), label, text);
    }
}

fn tint(style: &Style, index: usize) -> Color {
    let tints = style.palette.series();
    tints[index % tints.len()]
}
