use glam::Vec2;

use crate::element::{ButtonContent, Content, Element, IconButtonContent, Layout, Length, Size};
use crate::geometry::{Corners, Rect};
use crate::glass::GlassStyle;
use crate::icon::{self, Icon};
use crate::node::Runtime;
use crate::paint::Painter;
use crate::style::State;
use crate::text::{Align, TextStyle};
use crate::theme::{Role, Theme};

use super::{PaintInfo, padding, press_grow};

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
        style: None,
        inset: padding(theme),
        width,
        height: theme.metrics.control_height,
    }))
    .focus_ring(Corners::capsule())
    .cursor(crate::input::Cursor::Pointing)
}

pub fn button_enabled<'a, A>(
    theme: &Theme,
    label: &str,
    role: Role,
    width: Length,
    enabled: bool,
) -> Element<'a, A> {
    let element = button_width(theme, label, role, width);
    if enabled {
        element
    } else {
        element.enabled(false).press_disabled()
    }
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
        button.inset += 2.0;
    }
    element
}

pub fn glass_button<'a, A>(theme: &Theme, label: &str, style: GlassStyle) -> Element<'a, A> {
    let mut element = glass_button_core(theme, label, style);
    if let Content::Button(button) = &mut element.content {
        button.symbol = None;
    }
    element
}

pub fn glass_action_button<'a, A>(
    theme: &Theme,
    symbol: Icon,
    label: &str,
    style: GlassStyle,
) -> Element<'a, A> {
    let mut element = glass_button_core(theme, label, style);
    if let Content::Button(button) = &mut element.content {
        button.symbol = Some(symbol);
    }
    element
}

fn glass_button_core<'a, A>(theme: &Theme, label: &str, style: GlassStyle) -> Element<'a, A> {
    let height = theme.metrics.control_height + 8.0;
    Element::new(Layout {
        size: Size::new(Length::Auto, Length::Fixed(height)),
        ..Layout::row()
    })
    .with_content(Content::Button(ButtonContent {
        label: label.to_string(),
        role: Role::Control,
        font: theme.button_font(),
        symbol: None,
        style: Some(style.interactive()),
        inset: padding(theme) + 2.0,
        width: Length::Auto,
        height,
    }))
    .glass(style.interactive())
    .focus_ring(Corners::capsule())
    .cursor(crate::input::Cursor::Pointing)
}

pub fn icon_button<'a, A>(
    theme: &Theme,
    symbol: Icon,
    size: f32,
    role: Role,
    tooltip: &str,
) -> Element<'a, A> {
    let _ = theme;
    Element::new(Layout {
        size: Size::new(Length::Fixed(size), Length::Fixed(size)),
        ..Layout::row()
    })
    .with_content(Content::IconButton(IconButtonContent {
        symbol,
        size,
        role,
        style: None,
    }))
    .tooltip(tooltip)
    .focus_ring(Corners::capsule())
    .cursor(crate::input::Cursor::Pointing)
}

pub fn icon_button_disabled<'a, A>(
    theme: &Theme,
    symbol: Icon,
    size: f32,
    role: Role,
) -> Element<'a, A> {
    icon_button(theme, symbol, size, role, "")
        .enabled(false)
        .press_disabled()
}

pub fn glass_icon_button<'a, A>(
    theme: &Theme,
    group: Option<crate::element::Key>,
    symbol: Icon,
    size: f32,
    style: GlassStyle,
    tooltip: &str,
) -> Element<'a, A> {
    let mut element = icon_button(theme, symbol, size, Role::Control, tooltip);
    if let Content::IconButton(icon) = &mut element.content {
        icon.style = Some(style.interactive());
    }
    let element = element.glass(style.interactive());
    match group {
        Some(group) => element.glass_group(group),
        None => element,
    }
}

pub fn glass_icon_button_disabled<'a, A>(
    theme: &Theme,
    group: Option<crate::element::Key>,
    symbol: Icon,
    size: f32,
    style: GlassStyle,
) -> Element<'a, A> {
    let mut element = icon_button_disabled(theme, symbol, size, Role::Control);
    if let Content::IconButton(icon) = &mut element.content {
        icon.style = Some(style);
    }
    let element = element.glass(style);
    match group {
        Some(group) => element.glass_group(group),
        None => element,
    }
}
pub(super) fn measure_button(
    button: &ButtonContent,
    available: Vec2,
    text: &mut crate::text::Text,
    theme: &Theme,
) -> Vec2 {
    let _ = theme;
    let measured = text
        .layout(&button.label, button.font, 0.0, false, false, Align::Start)
        .size
        .x;
    let extra = if button.symbol.is_some() { 17.0 } else { 0.0 };
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

pub(super) fn paint_button(
    button: &ButtonContent,
    runtime: &Runtime,
    painter: &mut Painter,
    info: &PaintInfo<'_>,
) {
    let surface = surface(button.role, runtime, info);
    let body = if button.style.is_some() {
        info.glass.unwrap_or(info.rect)
    } else {
        info.rect.expand(press_grow(runtime.press.value))
    };
    painter.outlined(
        body,
        Corners::capsule(),
        surface.fill,
        surface.border,
        surface.shadow,
    );
    let style = TextStyle::new(button.font, surface.text);
    match button.symbol {
        Some(symbol) => {
            let glyph = Vec2::new(body.min.x + button.inset + 4.0, body.center().y);
            icon::paint(symbol, painter, glyph, 14.0, surface.text, 1.6);
            painter.text_in(
                Rect::from_min_max(
                    Vec2::new(glyph.x + 7.0, body.min.y),
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

pub(super) fn paint_icon_button(
    icon_content: &IconButtonContent,
    runtime: &Runtime,
    painter: &mut Painter,
    info: &PaintInfo<'_>,
) {
    let surface = surface(icon_content.role, runtime, info);
    let body = if icon_content.style.is_some() {
        info.glass.unwrap_or(info.rect)
    } else {
        info.rect.expand(press_grow(runtime.press.value))
    };
    if icon_content.style.is_none() {
        painter.outlined(
            body,
            Corners::capsule(),
            surface.fill,
            surface.border,
            surface.shadow,
        );
    }
    icon::paint(
        icon_content.symbol,
        painter,
        body.center(),
        (icon_content.size * 0.50).clamp(11.0, 17.0),
        surface.text,
        1.6,
    );
}

fn surface(role: Role, runtime: &Runtime, info: &PaintInfo<'_>) -> crate::style::Surface {
    if !info.enabled {
        return info.theme.surface(role, State::Disabled);
    }
    let rest = info.theme.surface(role, State::Rest);
    let hovered = info.theme.surface(role, State::Hover);
    let pressed = info.theme.surface(role, State::Pressed);
    let _ = runtime;
    rest.lerp(hovered, info.hover).lerp(pressed, info.press)
}
