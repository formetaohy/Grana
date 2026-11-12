use glam::Vec2;

use crate::content::{ButtonContent, Content, IconButtonContent};
use crate::element::{Element, Layout, Length, Size};
use crate::geometry::Rect;
use crate::icon::{self, Icon};
use crate::paint::Painter;
use crate::style::{State, Surface};
use crate::text::{Align, TextStyle};
use crate::theme::{Role, Theme};

use super::{PaintInfo, padding};

pub fn button<'a, A>(theme: &Theme, label: &str, role: Role) -> Element<'a, A> {
    button_width(theme, label, role, Length::Auto)
}

pub fn button_width<'a, A>(
    theme: &Theme,
    label: &str,
    role: Role,
    width: Length,
) -> Element<'a, A> {
    Element::new(Layout {
        size: Size::new(width, Length::Fixed(theme.metrics.control_height)),
        ..Layout::row()
    })
    .with_content(Content::Button(ButtonContent {
        label: label.to_string(),
        role,
        font: theme.button_font(),
        symbol: None,
        inset: padding(theme),
        width,
        height: theme.metrics.control_height,
    }))
}

pub fn button_enabled<'a, A>(
    theme: &Theme,
    label: &str,
    role: Role,
    width: Length,
    enabled: bool,
) -> Element<'a, A> {
    button_width(theme, label, role, width).enabled(enabled)
}

pub fn action_button<'a, A>(
    theme: &Theme,
    symbol: Icon,
    label: &str,
    role: Role,
) -> Element<'a, A> {
    let mut element = button_width(theme, label, role, Length::Auto);
    if let Content::Button(button) = &mut element.content {
        button.symbol = Some(symbol);
        button.inset += 1.0;
    }
    element
}

pub fn icon_button<'a, A>(symbol: Icon, size: f32, role: Role, tooltip: &str) -> Element<'a, A> {
    Element::new(Layout {
        size: Size::new(Length::Fixed(size), Length::Fixed(size)),
        ..Layout::row()
    })
    .with_content(Content::IconButton(IconButtonContent {
        symbol,
        size,
        role,
    }))
    .tooltip(tooltip)
}

pub(super) fn measure(
    button: &ButtonContent,
    available: Vec2,
    text: &mut crate::text::Text,
) -> Vec2 {
    let measured = text
        .layout(&button.label, button.font, 0.0, false, false, Align::Start)
        .size
        .x;
    let extra = if button.symbol.is_some() { 19.0 } else { 0.0 };
    let natural = measured + button.inset * 2.0 + extra;
    let width = match button.width {
        Length::Auto => natural,
        Length::Fill => available.x,
        Length::Fixed(value) => value,
        Length::Minimum(minimum) => natural.max(minimum),
        Length::Maximum(maximum) => natural.min(maximum),
    };
    Vec2::new(width, button.height)
}

pub(super) fn paint(button: &ButtonContent, painter: &mut Painter, info: &PaintInfo<'_>) {
    let surface = surface(button.role, info);
    let body = info.rect;
    painter.surface(body, surface);
    let style = TextStyle::new(button.font, surface.text);
    match button.symbol {
        Some(symbol) => {
            let glyph = Vec2::new(body.min.x + button.inset + 5.0, body.center().y);
            icon::paint(symbol, painter, glyph, 13.0, surface.text, 1.5);
            painter.text_in(
                Rect::from_min_max(
                    Vec2::new(glyph.x + 11.0, body.min.y),
                    Vec2::new(body.max.x - button.inset, body.max.y),
                ),
                &button.label,
                style.aligned(Align::Start),
                Align::Center,
            );
        }
        None => {
            painter.text_in(
                body,
                &button.label,
                style.aligned(Align::Center),
                Align::Center,
            );
        }
    }
}

pub(super) fn paint_icon(content: &IconButtonContent, painter: &mut Painter, info: &PaintInfo<'_>) {
    let surface = surface(content.role, info);
    let body = info.rect;
    painter.surface(body, surface);
    icon::paint(
        content.symbol,
        painter,
        body.center(),
        (content.size * 0.48).clamp(11.0, 16.0),
        surface.text,
        1.55,
    );
}

fn surface(role: Role, info: &PaintInfo<'_>) -> Surface {
    if !info.enabled {
        return info.theme.surface(role, State::Disabled);
    }
    let rest = info.theme.surface(role, State::Rest);
    let hovered = info.theme.surface(role, State::Hover);
    let pressed = info.theme.surface(role, State::Pressed);
    rest.lerp(hovered, info.hover).lerp(pressed, info.press)
}
