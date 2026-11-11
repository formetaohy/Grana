use glam::Vec2;
use grana_i18n::Language;
use grana_measure::Plot;
use grana_ui::{Align, Corners, Mark, Painter, Rect, TextStyle, Theme};

use crate::comparison::Comparison;
use crate::decor::{Decor, Gates, Highlight};
use crate::distribution::paint as distribution_paint;
use crate::histogram::paint as histogram_paint;
use crate::scatter::paint as scatter_paint;
use crate::series::{self, SeriesPage};
use crate::style::{DistributionStyle, Style};

pub const WIDTH: u32 = 960;
pub const HEIGHT: u32 = 600;

pub struct Heading {
    pub title: String,
    pub y: String,
    pub x: String,
    pub summary: String,
    pub empty: Option<String>,
    pub language: Language,
}

impl Heading {
    pub fn new(title: impl Into<String>, y: impl Into<String>, x: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            y: y.into(),
            x: x.into(),
            summary: String::new(),
            empty: None,
            language: Language::English,
        }
    }

    pub fn with_language(mut self, language: Language) -> Self {
        self.language = language;
        self
    }

    pub fn with_summary(mut self, summary: impl Into<String>) -> Self {
        self.summary = summary.into();
        self
    }

    pub fn with_empty(mut self, empty: impl Into<String>) -> Self {
        self.empty = Some(empty.into());
        self
    }
}

pub(crate) fn heading(painter: &mut Painter, rect: Rect, heading: &Heading, style: &Style) {
    let metrics = style.metrics;
    let palette = style.palette;
    if !heading.title.is_empty() {
        painter.text_in(
            metrics.title_band(rect),
            &heading.title,
            TextStyle::new(metrics.title, palette.text_primary),
            Align::Center,
        );
    }
    let label = TextStyle::new(metrics.label, palette.text_tertiary);
    let band = metrics.label_band(rect);
    painter.text_in(band, &heading.y, label, Align::Center);
    painter.text_in(band, &heading.x, label.aligned(Align::End), Align::Center);
    if !heading.summary.is_empty() {
        painter.text_in(
            metrics.summary_band(rect),
            &heading.summary,
            TextStyle::new(metrics.summary, palette.text_tertiary).truncating(),
            Align::Center,
        );
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
        let style = Style {
            language: self.heading.language,
            ..self.style
        };
        heading(painter, rect, self.heading, &style);
        let empty = self.heading.empty.as_deref();
        match self.plot {
            Plot::Histogram(histogram) => {
                histogram_paint(painter, rect, histogram, &style, &self.decor, empty)
            }
            Plot::Scatter(scatter) => {
                scatter_paint(painter, rect, scatter, &style, &self.decor, empty)
            }
            Plot::Distribution(distribution) => {
                distribution_paint(painter, rect, distribution, &style, &self.decor, empty)
            }
            Plot::Trend(trend) => series::paint(
                painter,
                rect,
                &series::SeriesChart::of_trend(trend),
                &style,
                &self.decor,
                empty,
            ),
            Plot::Ripley(ripley) => series::paint(
                painter,
                rect,
                &series::SeriesChart::of_ripley(ripley),
                &style,
                &self.decor,
                empty,
            ),
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

    fn marks(&self) -> Vec<Mark> {
        let mut recorder = grana_ui::Recorder::new(Theme::dark());
        let theme = *recorder.theme();
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
        recorder.marks(points, |painter| {
            painter.rect(page, Corners::ZERO, theme.palette.scene_backdrop);
            view.paint(painter, page);
        })
    }
}

pub enum Page {
    Plot(Box<Chart>),
    Comparison(Box<Comparison>),
    Series(Box<SeriesPage>),
}

impl Page {
    pub fn size(&self) -> [u32; 2] {
        match self {
            Self::Plot(chart) => chart.size(),
            Self::Comparison(comparison) => comparison.size(),
            Self::Series(series) => series.size(),
        }
    }

    pub fn distribution(&self) -> DistributionStyle {
        match self {
            Self::Plot(chart) => chart.distribution,
            Self::Comparison(comparison) => comparison.style,
            Self::Series(series) => series.style,
        }
    }

    pub fn paint(&self, painter: &mut Painter, rect: Rect, style: &Style, decor: &Decor<'_>) {
        match self {
            Self::Plot(chart) => {
                let view = View {
                    heading: &chart.heading,
                    plot: &chart.plot,
                    style: *style,
                    decor: Decor {
                        gates: chart.gate,
                        highlight: chart.highlight,
                        ..*decor
                    },
                };
                view.paint(painter, rect);
            }
            Self::Comparison(comparison) => comparison.paint(painter, rect, style, decor),
            Self::Series(series) => series.paint(painter, rect, style, decor),
        }
    }

    pub fn marks(&self) -> Vec<Mark> {
        match self {
            Self::Plot(chart) => chart.marks(),
            Self::Comparison(comparison) => comparison.marks(),
            Self::Series(series) => series.marks(),
        }
    }
}
