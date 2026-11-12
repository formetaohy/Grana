use glam::Vec2;
use grana_measure::DistributionSeries;
use grana_ui::{Align, Corners, Mark, Painter, Rect, TextStyle, Theme};

use crate::axis::Axis;
use crate::chart::{HEIGHT, Heading, WIDTH};
use crate::decor::Decor;
use crate::distribution::{self, Slot};
use crate::grid;
use crate::layout::{group_at, group_centre, slot as slot_width};
use crate::style::{DistributionStyle, Style};

const BRACKET_ROW: f32 = 17.0;
const BRACKET_TOP: f32 = 4.0;
const BRACKET_TICK: f32 = 4.0;
const LABEL: f32 = 54.0;

pub struct Category {
    pub label: String,
    pub values: Vec<f32>,
}

pub struct Note {
    pub from: usize,
    pub to: usize,
    pub mark: String,
}

pub struct Comparison {
    pub heading: Heading,
    pub categories: Vec<Category>,
    pub notes: Vec<Note>,
    pub style: DistributionStyle,
}

impl Comparison {
    fn extent(&self) -> (f32, f32) {
        let mut low = f32::INFINITY;
        let mut high = f32::NEG_INFINITY;
        for category in &self.categories {
            for value in &category.values {
                if value.is_finite() {
                    low = low.min(*value);
                    high = high.max(*value);
                }
            }
        }
        if !(low.is_finite() && high.is_finite()) {
            return (0.0, 1.0);
        }
        if high > low {
            return (low, high);
        }
        let padding = (0.5 * low.abs()).max(0.5);
        (low - padding, high + padding)
    }

    fn series(&self) -> Vec<Option<DistributionSeries>> {
        let range = self.extent();
        self.categories
            .iter()
            .map(|category| {
                DistributionSeries::of(
                    category
                        .values
                        .iter()
                        .map(|value| f64::from(*value))
                        .collect(),
                    range,
                )
            })
            .collect::<Vec<_>>()
    }
}

impl Comparison {
    pub fn size(&self) -> [u32; 2] {
        [WIDTH, HEIGHT]
    }

    pub(crate) fn marks(&self) -> Vec<Mark> {
        let mut recorder = grana_ui::Recorder::new(Theme::dark());
        let theme = *recorder.theme();
        let size = self.size();
        let points = Vec2::new(size[0] as f32, size[1] as f32);
        let page = Rect::from_min_size(Vec2::ZERO, points);
        let style = Style::page(&theme, self.style);
        recorder.marks(points, |painter| {
            painter.rect(page, Corners::ZERO, theme.palette.surface_dim);
            self.paint(painter, page, &style, &Decor::default());
        })
    }

    pub fn paint(&self, painter: &mut Painter, rect: Rect, style: &Style, decor: &Decor<'_>) {
        let style = Style {
            language: self.heading.language,
            ..*style
        };
        let metrics = style.metrics;
        let palette = style.palette;
        let style = &style;
        crate::chart::heading(painter, rect, &self.heading, style);
        let frame = metrics.frame(rect);
        if self.categories.is_empty() {
            if let Some(empty) = &self.heading.empty {
                painter.text_in(
                    frame,
                    empty,
                    distribution::empty_style(style),
                    Align::Center,
                );
            }
            return;
        }
        let brackets = self.notes.len() as f32 * BRACKET_ROW + BRACKET_TOP;
        let data = Rect::from_min_max(Vec2::new(frame.min.x, frame.min.y + brackets), frame.max);
        let range = self.extent();
        let y = Axis::new(range.0, range.1, 4);
        grid::axis_in(painter, rect, data, style, y);
        grid::slot_separators(painter, data, style, self.categories.len());
        let tick = metrics.tick_band(rect);
        let hovered = decor
            .pointer
            .and_then(|pointer| group_at(data, self.categories.len(), pointer));
        let width = (0.44 * slot_width(data, self.categories.len())).clamp(6.0, 34.0);
        let cold = TextStyle::new(metrics.caption, palette.outline)
            .aligned(Align::Center)
            .truncating();
        let hot = TextStyle::new(metrics.caption, palette.on_surface)
            .aligned(Align::Center)
            .truncating();
        let series = self.series();
        for (index, category) in self.categories.iter().enumerate() {
            let centre = group_centre(data, index, self.categories.len());
            if hovered == Some(index) {
                let half = 0.5 * slot_width(data, self.categories.len());
                painter.rect(
                    Rect::from_min_max(
                        Vec2::new(centre - half, data.min.y),
                        Vec2::new(centre + half, data.max.y),
                    ),
                    Corners::ZERO,
                    palette.on_surface.with_alpha(0.04),
                );
            }
            if let Some(series) = &series[index] {
                distribution::shape(
                    painter,
                    style,
                    series,
                    y,
                    Slot {
                        centre,
                        width,
                        frame: data,
                    },
                    true,
                );
            }
            painter.text_in(
                Rect::from_center_size(
                    Vec2::new(centre, tick.center().y),
                    Vec2::new(
                        (slot_width(data, self.categories.len()) - 4.0).max(LABEL),
                        metrics.tick_height,
                    ),
                ),
                &category.label,
                if hovered == Some(index) { hot } else { cold },
                Align::Center,
            );
        }
        let ink = palette.outline;
        let marks =
            TextStyle::new(metrics.caption, palette.on_surface_variant).aligned(Align::Center);
        for (row, note) in self.notes.iter().enumerate() {
            if note.from >= self.categories.len() || note.to >= self.categories.len() {
                continue;
            }
            let height = frame.min.y + BRACKET_TOP + row as f32 * BRACKET_ROW;
            let left = group_centre(frame, note.from.min(note.to), self.categories.len());
            let right = group_centre(frame, note.from.max(note.to), self.categories.len());
            if right - left < 4.0 {
                continue;
            }
            painter.segment(Vec2::new(left, height), Vec2::new(right, height), 1.0, ink);
            for pixel in [left, right] {
                painter.segment(
                    Vec2::new(pixel, height),
                    Vec2::new(pixel, height + BRACKET_TICK),
                    1.0,
                    ink,
                );
            }
            painter.text_in(
                Rect::from_center_size(
                    Vec2::new(0.5 * (left + right), height - 0.5 * BRACKET_ROW),
                    Vec2::new((right - left).max(40.0), BRACKET_ROW),
                ),
                &note.mark,
                marks,
                Align::Center,
            );
        }
    }
}
