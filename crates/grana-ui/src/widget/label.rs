use glam::Vec2;

use crate::color::Color;
use crate::content::{
    BadgeContent, ColorSwatchContent, Content, LabelledContent, SpinnerContent, TextContent,
};
use crate::element::{Element, Layout, Length, Size};
use crate::geometry::Corners;
use crate::paint::Painter;
use crate::style::{Border, Fill, Shadow};
use crate::text::{Align, TextStyle};
use crate::theme::{Level, Theme, TypeStyle};

use super::PaintInfo;

pub fn caption<'a, A>(theme: &Theme, text: &str) -> Element<'a, A> {
    level(theme, Level::Caption, text)
}

pub fn weak<'a, A>(theme: &Theme, text: &str) -> Element<'a, A> {
    let mut element = level(theme, Level::Caption, text);
    if let Content::Text(content) = &mut element.content {
        content.style.wrap = true;
    }
    element
}

fn level<'a, A>(theme: &Theme, level: Level, text: &str) -> Element<'a, A> {
    Element::new(Layout {
        size: Size::AUTO,
        ..Layout::row()
    })
    .with_content(Content::Text(TextContent {
        text: text.to_string(),
        style: TextStyle::new(theme.font(level), theme.text(level)),
    }))
}

pub fn badge<'a, A>(label: &str, tint: Color) -> Element<'a, A> {
    Element::new(Layout {
        size: Size::new(Length::Auto, Length::Fixed(19.0)),
        ..Layout::row()
    })
    .with_content(Content::Badge(BadgeContent {
        label: label.to_string(),
        tint,
        font: TypeStyle {
            size: 10.0,
            weight: 600.0,
            tracking: 0.4,
            leading: 13.0,
        },
    }))
}

pub fn labelled<'a, A>(theme: &Theme, label: &str, value: &str) -> Element<'a, A> {
    Element::new(Layout {
        size: Size::new(Length::Fill, Length::Fixed(22.0)),
        ..Layout::row()
    })
    .with_content(Content::Labelled(LabelledContent {
        label: label.to_string(),
        value: value.to_string(),
        label_font: theme.font(Level::Caption),
        value_font: theme.font(Level::Monospace),
    }))
}

pub fn spinner<'a, A>(size: f32) -> Element<'a, A> {
    Element::new(Layout {
        size: Size::new(Length::Fixed(size), Length::Fixed(size)),
        ..Layout::row()
    })
    .with_content(Content::Spinner(SpinnerContent { size }))
}

pub fn color_swatch<'a, A>(color: [f32; 3], size: f32) -> Element<'a, A> {
    Element::new(Layout {
        size: Size::new(Length::Fixed(size), Length::Fixed(size)),
        ..Layout::row()
    })
    .with_content(Content::ColorSwatch(ColorSwatchContent { color, size }))
}

pub(super) fn measure_badge(badge: &BadgeContent, text: &mut crate::text::Text) -> Vec2 {
    let width = text
        .layout(&badge.label, badge.font, 0.0, false, false, Align::Start)
        .size
        .x;
    Vec2::new(width + 14.0, 19.0)
}

pub(super) fn paint_badge(badge: &BadgeContent, painter: &mut Painter, info: &PaintInfo<'_>) {
    let palette = info.theme.palette;
    painter.outlined(
        info.rect,
        Corners::capsule(),
        Fill::Solid(palette.material_fill.amplify(0.7)),
        Border {
            width: 1.0,
            color: palette.material_border,
        },
        Shadow::NONE,
    );
    painter.text_in(
        info.rect,
        &badge.label,
        TextStyle::new(badge.font, badge.tint.with_alpha(0.92)),
        Align::Center,
    );
}

pub(super) fn paint_labelled(
    labelled: &LabelledContent,
    painter: &mut Painter,
    info: &PaintInfo<'_>,
) {
    let palette = info.theme.palette;
    let width = info.rect.width();
    let label_width = (width * 0.38).clamp(64.0, 128.0);
    painter.text_in(
        crate::geometry::Rect::from_min_size(
            info.rect.min,
            Vec2::new(label_width, info.rect.height()),
        ),
        &labelled.label,
        TextStyle::new(labelled.label_font, palette.text_tertiary).truncating(),
        Align::Center,
    );
    painter.text_in(
        crate::geometry::Rect::from_min_size(
            Vec2::new(info.rect.min.x + label_width, info.rect.min.y),
            Vec2::new(width - label_width, info.rect.height()),
        ),
        &labelled.value,
        TextStyle::new(labelled.value_font, palette.text_secondary)
            .aligned(Align::End)
            .truncating(),
        Align::Center,
    );
}

pub(super) fn paint_spinner(spinner: &SpinnerContent, painter: &mut Painter, info: &PaintInfo<'_>) {
    let palette = info.theme.palette;
    let center = info.rect.center();
    let radius = spinner.size * 0.38;
    painter.ring(
        center,
        radius,
        1.6,
        0.0,
        std::f32::consts::TAU,
        palette.track.amplify(0.5),
    );
    painter.ring(center, radius, 1.6, info.time * 5.0, 1.7, palette.accent);
}

pub(super) fn paint_color_swatch(
    swatch: &ColorSwatchContent,
    painter: &mut Painter,
    info: &PaintInfo<'_>,
) {
    painter.rect(
        info.rect,
        Corners::all(5.0),
        Color::from_rgb_f32(swatch.color),
    );
    painter.outlined(
        info.rect,
        Corners::all(5.0),
        Fill::None,
        Border {
            width: 1.0,
            color: Color::white_alpha(0.22 + 0.35 * info.hover),
        },
        Shadow::NONE,
    );
}
