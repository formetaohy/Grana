use glam::Vec2;
use grana_i18n::Language;
use grana_ui::{Level, Palette, Rect, Theme, TypeStyle};

use crate::layout::Insets;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DistributionStyle {
    Box,
    Violin,
}

impl DistributionStyle {
    pub const ALL: [Self; 2] = [Self::Box, Self::Violin];

    pub fn name(self) -> &'static str {
        match self {
            Self::Box => "Box",
            Self::Violin => "Violin",
        }
    }

    pub fn index(self) -> usize {
        match self {
            Self::Box => 0,
            Self::Violin => 1,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    pub insets: Insets,
    pub pad: f32,
    pub title_height: f32,
    pub label_height: f32,
    pub tick_height: f32,
    pub tick_width: f32,
    pub summary_height: f32,
    pub tick: TypeStyle,
    pub caption: TypeStyle,
    pub label: TypeStyle,
    pub title: TypeStyle,
    pub summary: TypeStyle,
}

impl Metrics {
    pub fn card(theme: &Theme) -> Self {
        Self {
            insets: Insets::new(44.0, 14.0, 28.0, 40.0),
            pad: 12.0,
            title_height: 0.0,
            label_height: 28.0,
            tick_height: 18.0,
            tick_width: 72.0,
            summary_height: 22.0,
            tick: theme.font(Level::Monospace),
            caption: theme.font(Level::Caption),
            label: TypeStyle::new(10.5, 500.0, 14.0),
            title: TypeStyle::new(10.5, 500.0, 14.0),
            summary: TypeStyle::new(10.5, 400.0, 14.0),
        }
    }

    pub fn page() -> Self {
        Self {
            insets: Insets::new(78.0, 34.0, 74.0, 52.0),
            pad: 34.0,
            title_height: 48.0,
            label_height: 26.0,
            tick_height: 20.0,
            tick_width: 112.0,
            summary_height: 32.0,
            tick: TypeStyle::new(10.5, 400.0, 14.0),
            caption: TypeStyle::new(10.5, 400.0, 14.0),
            label: TypeStyle::new(11.5, 500.0, 16.0),
            title: TypeStyle::new(19.0, 600.0, 26.0),
            summary: TypeStyle::new(11.5, 400.0, 16.0),
        }
    }

    pub fn frame(self, rect: Rect) -> Rect {
        self.insets.frame(rect)
    }

    pub fn title_band(self, rect: Rect) -> Rect {
        Rect::from_min_max(
            Vec2::new(rect.min.x + self.pad, rect.min.y),
            Vec2::new(
                rect.max.x - self.pad,
                rect.min.y + self.title_height.max(0.0),
            ),
        )
    }

    pub fn label_band(self, rect: Rect) -> Rect {
        Rect::from_min_max(
            Vec2::new(rect.min.x + self.pad, rect.min.y + self.title_height),
            Vec2::new(rect.max.x - self.pad, rect.min.y + self.insets.top),
        )
    }

    pub fn tick_band(self, rect: Rect) -> Rect {
        let frame = self.frame(rect);
        Rect::from_min_max(
            Vec2::new(frame.min.x, frame.max.y),
            Vec2::new(frame.max.x, frame.max.y + self.tick_height),
        )
    }

    pub fn summary_band(self, rect: Rect) -> Rect {
        Rect::from_min_max(
            Vec2::new(rect.min.x + self.pad, rect.max.y - self.summary_height),
            Vec2::new(rect.max.x - self.pad, rect.max.y),
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Style {
    pub palette: Palette,
    pub metrics: Metrics,
    pub distribution: DistributionStyle,
    pub language: Language,
}

impl Style {
    pub fn card(theme: &Theme, distribution: DistributionStyle) -> Self {
        Self {
            palette: theme.palette,
            metrics: Metrics::card(theme),
            distribution,
            language: Language::English,
        }
    }

    pub fn page(theme: &Theme, distribution: DistributionStyle) -> Self {
        Self {
            palette: theme.palette,
            metrics: Metrics::page(),
            distribution,
            language: Language::English,
        }
    }

    pub fn with_language(mut self, language: Language) -> Self {
        self.language = language;
        self
    }
}
