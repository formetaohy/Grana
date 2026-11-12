use glam::Vec2;

use crate::content::{ButtonContent, Content, IconButtonContent};
use crate::element::{Element, Key, Layout, Length, Size};
use crate::geometry::{Corners, Rect};
use crate::icon::{self, Icon};
use crate::paint::Painter;
use crate::style::{State, Surface};
use crate::text::{Align, TextStyle};
use crate::theme::{Role, Theme};

use super::{PaintInfo, padding};

pub struct Split<'a, A> {
    pub symbol: Option<Icon>,
    pub label: &'a str,
    pub role: Role,
    pub leading_key: Key,
    pub leading_action: A,
    pub trailing_key: Key,
    pub trailing_action: A,
    pub open: bool,
}

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
        corners: None,
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
        corners: None,
    }))
    .tooltip(tooltip)
}

pub fn split_button<'a, A: Clone>(theme: &Theme, spec: Split<'_, A>) -> Element<'a, A> {
    let inner = theme.metrics.corner_xs;
    let lead_corners = Corners::split_leading(inner);
    let trail_corners = if spec.open {
        Corners::capsule()
    } else {
        Corners::split_trailing(inner)
    };
    let leading = match spec.symbol {
        Some(icon) => action_button(theme, icon, spec.label, spec.role),
        None => button_width(theme, spec.label, spec.role, Length::Auto),
    }
    .shaped(lead_corners)
    .key(spec.leading_key)
    .press(spec.leading_action);
    let trailing_icon = if spec.open {
        Icon::ChevronUp
    } else {
        Icon::ChevronDown
    };
    let trailing = icon_button(trailing_icon, theme.metrics.control_height, spec.role, "")
        .shaped(trail_corners)
        .key(spec.trailing_key)
        .press(spec.trailing_action);
    crate::element::row()
        .gap(theme.metrics.group_gap)
        .align(crate::text::Align::Center)
        .children([leading, trailing])
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
    let extra = if button.symbol.is_some() { 20.0 } else { 0.0 };
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
    let mut surface = surface(button.role, info);
    let body = info.rect;
    if let Some(custom) = button.corners {
        let resolved = custom.resolved(body.size());
        let pressed = Corners::all(info.theme.metrics.corner_sm).resolved(body.size());
        surface.corners = resolved.lerp(pressed, info.press);
    } else {
        let rest = Corners::capsule().resolved(body.size());
        let pressed = Corners::all(info.theme.metrics.corner_sm).resolved(body.size());
        surface.corners = rest.lerp(pressed, info.press);
    }
    painter.surface(body, surface);
    let optical = (surface.corners.top_left - surface.corners.top_right) * 0.12;
    let style = TextStyle::new(button.font, surface.text);
    match button.symbol {
        Some(symbol) => {
            let text_w = painter.measure(&button.label, style).x;
            let content_w = 18.5 + text_w;
            let start_x =
                (body.center().x - content_w * 0.5 + optical).max(body.min.x + button.inset);
            let glyph = Vec2::new(start_x + 5.0, body.center().y);
            icon::paint(symbol, painter, glyph, 13.5, surface.text, 1.6);
            painter.text_in(
                Rect::from_min_max(
                    Vec2::new(glyph.x + 12.0, body.min.y),
                    Vec2::new(body.max.x - button.inset + optical, body.max.y),
                ),
                &button.label,
                style.aligned(Align::Start),
                Align::Center,
            );
        }
        None => {
            painter.text_in(
                body.translate(Vec2::new(optical, 0.0)),
                &button.label,
                style.aligned(Align::Center),
                Align::Center,
            );
        }
    }
}

pub(super) fn paint_icon(content: &IconButtonContent, painter: &mut Painter, info: &PaintInfo<'_>) {
    let mut surface = surface(content.role, info);
    let body = info.rect;
    if let Some(custom) = content.corners {
        let resolved = custom.resolved(body.size());
        let pressed = Corners::all(info.theme.metrics.corner_sm).resolved(body.size());
        surface.corners = resolved.lerp(pressed, info.press);
    } else {
        let rest = Corners::capsule().resolved(body.size());
        let pressed = Corners::all(info.theme.metrics.corner_sm).resolved(body.size());
        surface.corners = rest.lerp(pressed, info.press);
    }
    painter.surface(body, surface);
    let optical = (surface.corners.top_left - surface.corners.top_right) * 0.11;
    icon::paint(
        content.symbol,
        painter,
        body.center() + Vec2::new(optical, 0.0),
        (content.size * 0.48).clamp(11.5, 16.0),
        surface.text,
        1.6,
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
