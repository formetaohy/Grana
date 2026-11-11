use glam::Vec2;
use grana_measure::Plot;
use grana_ui::{Align, Corners, Mark, Painter, Rect, TextStyle, Ui};

use crate::comparison::Comparison;
use crate::decor::{Decor, Gates, Highlight};
use crate::distribution::paint as distribution_paint;
use crate::histogram::paint as histogram_paint;
use crate::scatter::paint as scatter_paint;
use crate::style::{DistributionStyle, Style};
use crate::trend::paint as trend_paint;

pub const WIDTH: u32 = 960;
pub const HEIGHT: u32 = 600;

pub struct Heading {
    pub title: String,
    pub y: String,
    pub x: String,
    pub summary: String,
}

impl Heading {
    pub fn new(title: impl Into<String>, y: impl Into<String>, x: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            y: y.into(),
            x: x.into(),
            summary: String::new(),
        }
    }

    pub fn with_summary(mut self, summary: impl Into<String>) -> Self {
        self.summary = summary.into();
        self
    }
}

pub struct View<'a> {
    pub heading: &'a Heading,
    pub plot: &'a Plot,
    pub style: Style,
    pub decor: Decor<'a>,
}

impl View<'_> {
    pub fn paint(&self, painter: &mut Painter, rect: Rect) {
        let metrics = self.style.metrics;
        let palette = self.style.palette;
        if !self.heading.title.is_empty() {
            painter.text_in(
                metrics.title_band(rect),
                &self.heading.title,
                TextStyle::new(metrics.title, palette.text_primary),
                Align::Center,
            );
        }
        let label = TextStyle::new(metrics.label, palette.text_tertiary);
        let band = metrics.label_band(rect);
        painter.text_in(band, &self.heading.y, label, Align::Center);
        painter.text_in(
            band,
            &self.heading.x,
            label.aligned(Align::End),
            Align::Center,
        );
        if !self.heading.summary.is_empty() {
            painter.text_in(
                metrics.summary_band(rect),
                &self.heading.summary,
                TextStyle::new(metrics.summary, palette.text_tertiary).truncating(),
                Align::Center,
            );
        }
        match self.plot {
            Plot::Histogram(histogram) => {
                histogram_paint(painter, rect, histogram, &self.style, &self.decor)
            }
            Plot::Scatter(scatter) => {
                scatter_paint(painter, rect, scatter, &self.style, &self.decor)
            }
            Plot::Distribution(distribution) => {
                distribution_paint(painter, rect, distribution, &self.style, &self.decor)
            }
            Plot::Trend(trend) => trend_paint(painter, rect, trend, &self.style, &self.decor),
        }
    }
}

pub struct Chart {
    pub heading: Heading,
    pub plot: Plot,
    pub gate: Gates,
    pub highlight: Option<Highlight>,
    pub distribution: DistributionStyle,
}

impl Chart {
    pub fn size(&self) -> [u32; 2] {
        [WIDTH, HEIGHT]
    }

    fn marks<A: Clone>(&self, ui: &mut Ui<A>) -> Vec<Mark> {
        let theme = *ui.theme();
        let size = self.size();
        let points = Vec2::new(size[0] as f32, size[1] as f32);
        let page = Rect::from_min_size(Vec2::ZERO, points);
        let style = Style::page(&theme, self.distribution);
        let view = View {
            heading: &self.heading,
            plot: &self.plot,
            style,
            decor: Decor {
                gates: self.gate,
                highlight: self.highlight,
                ..Decor::default()
            },
        };
        ui.figure_marks(points, 1.0, |painter| {
            painter.rect(page, Corners::ZERO, theme.palette.scene_backdrop);
            view.paint(painter, page);
        })
    }
}

pub enum Page {
    Plot(Box<Chart>),
    Comparison(Box<Comparison>),
}

impl Page {
    pub fn size(&self) -> [u32; 2] {
        match self {
            Self::Plot(chart) => chart.size(),
            Self::Comparison(comparison) => comparison.size(),
        }
    }

    pub fn marks<A: Clone>(&self, ui: &mut Ui<A>) -> Vec<Mark> {
        match self {
            Self::Plot(chart) => chart.marks(ui),
            Self::Comparison(comparison) => comparison.marks(ui),
        }
    }
}
