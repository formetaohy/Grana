use glam::Vec2;
use grana_i18n::Language;
use grana_measure::{Ripley, Trend};
use grana_profile::Profile;
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

const DOTS: usize = 64;

const NOTE_GAP: f32 = 11.0;

const NOTE_REACH: f32 = 18.0;

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
    pub tint: Option<[f32; 3]>,
    pub points: Vec<CurvePoint>,
    pub members: Vec<Vec<CurvePoint>>,
}

impl Curve {
    pub fn new(label: impl Into<String>, points: Vec<CurvePoint>) -> Self {
        Self {
            label: label.into(),
            tint: None,
            points,
            members: Vec::new(),
        }
    }

    pub fn with_tint(mut self, tint: [f32; 3]) -> Self {
        assert!(
            tint.iter().all(|value| (0.0..=1.0).contains(value)),
            "a curve carries an srgb tint"
        );
        self.tint = Some(tint);
        self
    }

    pub fn with_members(mut self, members: Vec<Vec<CurvePoint>>) -> Self {
        self.members = members;
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BandPoint {
    pub x: f32,
    pub lower: f32,
    pub upper: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Band {
    pub label: String,
    pub points: Vec<BandPoint>,
}

impl Band {
    pub fn new(label: impl Into<String>, points: Vec<BandPoint>) -> Self {
        Self {
            label: label.into(),
            points,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    pub x: f32,
    pub y: f32,
    pub mark: String,
}

pub struct SeriesChart {
    pub x: Axis,
    pub y: Axis,
    pub curves: Vec<Curve>,
    pub bands: Vec<Band>,
    pub notes: Vec<Note>,
    pub outside: Vec<(f32, f32)>,
}

impl SeriesChart {
    pub fn of_trend(trend: &Trend) -> Self {
        let frames = trend.frames().max(2) as f32;
        Self {
            x: Axis::frames(1.0, frames, 5),
            y: Axis::new(trend.low(), trend.high(), 4),
            curves: vec![Curve::new(
                String::new(),
                trend
                    .points()
                    .iter()
                    .map(|point| CurvePoint {
                        x: point.time() as f32 + 1.0,
                        value: point.value(),
                        lower: point.lower(),
                        upper: point.upper(),
                    })
                    .collect(),
            )],
            bands: Vec::new(),
            notes: Vec::new(),
            outside: Vec::new(),
        }
    }

    pub fn of_ripley(ripley: &Ripley, language: Language) -> Self {
        let curve = Curve::new(
            grana_i18n::measure::ripley_curve(language),
            ripley
                .points()
                .iter()
                .map(|point| CurvePoint {
                    x: point.radius(),
                    value: point.value(),
                    lower: point.lower(),
                    upper: point.upper(),
                })
                .collect(),
        );
        let points = ripley
            .points()
            .iter()
            .filter_map(|point| {
                let (lower, upper) = point.envelope()?;
                Some(BandPoint {
                    x: point.radius(),
                    lower,
                    upper,
                })
            })
            .collect::<Vec<_>>();
        let bands = (!points.is_empty())
            .then(|| {
                Band::new(
                    grana_i18n::measure::ripley_envelope(language, ripley.simulations()),
                    points,
                )
            })
            .into_iter()
            .collect();
        let outside = ripley
            .points()
            .iter()
            .filter(|point| point.beyond())
            .map(|point| (point.radius(), point.value()))
            .collect();
        Self {
            x: Axis::new(0.0, ripley.radius(), 5),
            y: Axis::new(ripley.low(), ripley.high(), 4),
            curves: vec![curve],
            bands,
            notes: Vec::new(),
            outside,
        }
    }

    pub fn of_profile(profile: &Profile, tints: &[[f32; 3]]) -> Self {
        let mut low = f32::INFINITY;
        let mut high = f32::NEG_INFINITY;
        for sample in profile.samples() {
            for value in sample.values() {
                if value.is_finite() {
                    low = low.min(*value);
                    high = high.max(*value);
                }
            }
        }
        if !(low.is_finite() && high.is_finite()) {
            low = 0.0;
            high = 1.0;
        } else if high <= low {
            let padding = (0.5 * low.abs()).max(0.5);
            low -= padding;
            high += padding;
        }
        let length = profile.length().max(1.0e-6);
        let curves = (0..profile.names().len())
            .map(|channel| {
                let points = profile
                    .samples()
                    .iter()
                    .filter_map(|sample| {
                        let value = *sample.values().get(channel)?;
                        value.is_finite().then_some(CurvePoint {
                            x: sample.offset(),
                            value,
                            lower: value,
                            upper: value,
                        })
                    })
                    .collect::<Vec<_>>();
                let curve = Curve::new(profile.names()[channel].clone(), points);
                match tints.get(channel).copied() {
                    Some(tint) => curve.with_tint(tint),
                    None => curve,
                }
            })
            .collect();
        Self {
            x: Axis::new(0.0, length, 5),
            y: Axis::new(low, high, 4),
            curves,
            bands: Vec::new(),
            notes: Vec::new(),
            outside: Vec::new(),
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
    pub fn of_profile(
        profile: &Profile,
        tints: &[[f32; 3]],
        heading: Heading,
        style: DistributionStyle,
    ) -> Self {
        Self {
            heading,
            chart: SeriesChart::of_profile(profile, tints),
            style,
        }
    }

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
            painter.rect(page, Corners::ZERO, theme.palette.surface_dim);
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
    let accent = palette.primary;
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
    let neutral = palette.outline;
    let projected_bands = chart
        .bands
        .iter()
        .map(|band| {
            band.points
                .iter()
                .map(|point| {
                    (
                        chart.x.x(frame, point.x),
                        chart.y.y(frame, point.lower),
                        chart.y.y(frame, point.upper),
                    )
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    for points in &projected_bands {
        for pair in points.windows(2) {
            let band = neutral.with_alpha(0.11);
            painter.triangle(
                Vec2::new(pair[0].0, pair[0].1),
                Vec2::new(pair[1].0, pair[1].1),
                Vec2::new(pair[1].0, pair[1].2),
                band,
            );
            painter.triangle(
                Vec2::new(pair[0].0, pair[0].1),
                Vec2::new(pair[0].0, pair[0].2),
                Vec2::new(pair[1].0, pair[1].2),
                band,
            );
        }
        let edge = neutral.with_alpha(0.34);
        for pair in points.windows(2) {
            painter.segment(
                Vec2::new(pair[0].0, pair[0].1),
                Vec2::new(pair[1].0, pair[1].1),
                0.6,
                edge,
            );
            painter.segment(
                Vec2::new(pair[0].0, pair[0].2),
                Vec2::new(pair[1].0, pair[1].2),
                0.6,
                edge,
            );
        }
    }
    let single = chart.curves.len() == 1;
    let projected = chart
        .curves
        .iter()
        .enumerate()
        .map(|(index, curve)| {
            (
                tint(style, index, curve),
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
                curve
                    .members
                    .iter()
                    .map(|member| {
                        member
                            .iter()
                            .map(|point| (chart.x.x(frame, point.x), chart.y.y(frame, point.value)))
                            .collect::<Vec<_>>()
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<Vec<_>>();
    for (color, points, _) in &projected {
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
    for (color, _, members) in &projected {
        let faint = color.with_alpha(0.20);
        for member in members {
            for pair in member.windows(2) {
                painter.segment(
                    Vec2::new(pair[0].0, pair[0].1),
                    Vec2::new(pair[1].0, pair[1].1),
                    0.9,
                    faint,
                );
            }
        }
    }
    for (color, points, _) in &projected {
        let dots = single && points.len() <= DOTS;
        if dots {
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
        if dots || points.len() < 2 {
            for point in points {
                painter.rect(
                    Rect::from_center_size(Vec2::new(point.0, point.1), Vec2::splat(2.6)),
                    Corners::capsule(),
                    color.with_alpha(0.9),
                );
            }
        }
    }
    for (x, y) in &chart.outside {
        let centre = Vec2::new(chart.x.x(frame, *x), chart.y.y(frame, *y));
        painter.outlined(
            Rect::from_center_size(centre, Vec2::splat(7.5)),
            Corners::capsule(),
            Fill::Solid(palette.surface_dim.with_alpha(0.45)),
            Border {
                width: 1.3,
                color: accent,
            },
            Shadow::NONE,
        );
    }
    paint_notes(painter, frame, chart, style);
    legend(painter, frame, style, chart);
    if let Some(Highlight::Point(point)) = decor.highlight {
        let centre = Vec2::new(chart.x.x(frame, point.x), chart.y.y(frame, point.y));
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
    if let Some(pointer) = decor.pointer {
        painter.segment(
            Vec2::new(pointer.x, frame.min.y),
            Vec2::new(pointer.x, frame.max.y),
            0.6,
            palette.on_surface.with_alpha(0.12),
        );
    }
}

fn paint_notes(painter: &mut Painter, frame: Rect, chart: &SeriesChart, style: &Style) {
    if chart.notes.is_empty() {
        return;
    }
    let text = TextStyle::new(style.metrics.caption, style.palette.on_surface_variant)
        .aligned(Align::Center);
    let mut placed = Vec::<(f32, f32)>::new();
    for note in &chart.notes {
        let x = chart.x.x(frame, note.x);
        let mut y = chart.y.y(frame, note.y);
        while placed
            .iter()
            .any(|(other, height)| (other - x).abs() < NOTE_REACH && (height - y).abs() < NOTE_GAP)
        {
            y -= NOTE_GAP;
        }
        placed.push((x, y));
        painter.text_in(
            Rect::from_center_size(
                Vec2::new(x, y - 0.5 * NOTE_GAP),
                Vec2::new(NOTE_REACH * 2.0, NOTE_GAP),
            ),
            &note.mark,
            text,
            Align::Center,
        );
    }
}

fn legend(painter: &mut Painter, frame: Rect, style: &Style, chart: &SeriesChart) {
    let metrics = style.metrics;
    let text = TextStyle::new(metrics.caption, style.palette.outline);
    let mut entries = Vec::new();
    let mut pen = LEGEND_INSET;
    let mut row = LEGEND_INSET;
    let row_height = LEGEND_HEIGHT + 4.0;
    let mut width = 0.0f32;
    let mut push = |painter: &mut Painter, label: &str, color: Color, band: bool| {
        if label.is_empty() {
            return;
        }
        let measured = painter.measure(label, text).x;
        if pen > LEGEND_INSET && pen + 16.0 + measured > frame.width() - 2.0 * LEGEND_INSET {
            pen = LEGEND_INSET;
            row += row_height;
        }
        if row + LEGEND_HEIGHT > frame.height() - LEGEND_INSET {
            return;
        }
        entries.push((label.to_string(), color, band, pen, row, measured));
        pen += 16.0 + measured + LEGEND_GAP;
        width = width.max(pen - LEGEND_GAP + LEGEND_INSET);
    };
    for (index, curve) in chart.curves.iter().enumerate() {
        let color = tint(style, index, curve);
        push(painter, &curve.label, color, false);
    }
    let neutral = style.palette.outline;
    for band in &chart.bands {
        push(painter, &band.label, neutral, true);
    }
    if entries.is_empty() {
        return;
    }
    let height = row + LEGEND_HEIGHT + LEGEND_INSET;
    painter.rect(
        Rect::from_min_size(frame.min + Vec2::splat(2.0), Vec2::new(width, height)),
        Corners::all(6.0),
        style.palette.surface_dim.with_alpha(0.72),
    );
    for (label, color, band, x, y, _) in entries {
        let swatch = Rect::from_center_size(
            frame.min + Vec2::new(x + 2.0 + 6.0, y + 2.0 + 0.5 * LEGEND_HEIGHT),
            Vec2::new(12.0, if band { 7.0 } else { 3.0 }),
        );
        if band {
            painter.outlined(
                swatch,
                Corners::all(2.0),
                Fill::Solid(color.with_alpha(0.16)),
                Border {
                    width: 0.6,
                    color: color.with_alpha(0.45),
                },
                Shadow::NONE,
            );
        } else {
            painter.rect(swatch, Corners::capsule(), color);
        }
        painter.text_at(frame.min + Vec2::new(x + 2.0 + 16.0, y + 2.0), &label, text);
    }
}

fn tint(style: &Style, index: usize, curve: &Curve) -> Color {
    match curve.tint {
        Some(tint) => Color::from_rgb_f32(tint),
        None => {
            let tints = style.palette.series();
            tints[index % tints.len()]
        }
    }
}
