use glam::Vec2;

use crate::color::Color;
use crate::geometry::{Corners, Rect};
use crate::glass::GlassStyle;
use crate::icon::{self, Icon};
use crate::motion::Motion;
use crate::style::Border;
use crate::text::{Align, TextStyle};
use crate::theme::Role;
use crate::ui::{GlassGroup, Length, Response, Ui};

use super::{button_font, padding, pressed_inset};

pub fn button(ui: &mut Ui, id: &str, label: &str, role: Role) -> bool {
    button_with(ui, id, label, role, Length::Auto).clicked
}

pub fn button_with(ui: &mut Ui, id: &str, label: &str, role: Role, width: Length) -> Response {
    let key = ui.id(id);
    let metrics = ui.theme().metrics;
    let font = button_font(ui);
    let measured = ui.measure_text(label, TextStyle::new(font, Color::WHITE));
    let inset = padding(ui);
    let natural = measured.x + inset * 2.0;
    let width = match width {
        Length::Auto => natural,
        Length::Fill => ui.available().x,
        Length::Fixed(value) => value,
        Length::Minimum(minimum) => natural.max(minimum),
        Length::Maximum(maximum) => natural.min(maximum),
    };
    let rect = ui.allocate(Vec2::new(width, metrics.control_height));
    let mut response = ui.interact(key, rect);
    if ui.focusable(key, rect, Corners::capsule()) && ui.activated(key) {
        response.clicked = true;
    }
    let press = ui.animate(
        key ^ 0x51,
        if response.active { 1.0 } else { 0.0 },
        Motion::quick(),
    );
    let surface = ui.surface(key, role, &response);
    let body = rect.shrink(pressed_inset(press));
    let mut painter = ui.painter();
    painter.outlined(
        body,
        Corners::capsule(),
        surface.fill,
        surface.border,
        surface.shadow,
    );
    painter.text_in(
        body,
        label,
        TextStyle::new(font, surface.text).aligned(Align::Center),
        Align::Center,
    );
    response
}

pub fn action_button(ui: &mut Ui, id: &str, symbol: Icon, label: &str, role: Role) -> Response {
    let key = ui.id(id);
    let metrics = ui.theme().metrics;
    let font = button_font(ui);
    let measured = ui.measure_text(label, TextStyle::new(font, Color::WHITE));
    let inset = padding(ui);
    let width = measured.x + inset * 2.0 + 17.0;
    let rect = ui.allocate(Vec2::new(width, metrics.control_height));
    let mut response = ui.interact(key, rect);
    if ui.focusable(key, rect, Corners::capsule()) && ui.activated(key) {
        response.clicked = true;
    }
    let press = ui.animate(
        key ^ 0x61,
        if response.active { 1.0 } else { 0.0 },
        Motion::quick(),
    );
    let surface = ui.surface(key, role, &response);
    let body = rect.shrink(pressed_inset(press));
    let glyph = Rect::from_center_size(
        Vec2::new(body.min.x + inset + 4.0, body.center().y),
        Vec2::splat(14.0),
    );
    let mut painter = ui.painter();
    painter.outlined(
        body,
        Corners::capsule(),
        surface.fill,
        surface.border,
        surface.shadow,
    );
    icon::paint(
        symbol,
        &mut painter,
        glyph.center(),
        14.0,
        surface.text,
        1.6,
    );
    painter.text_in(
        Rect::from_min_max(
            Vec2::new(glyph.max.x + 7.0, body.min.y),
            Vec2::new(body.max.x - inset, body.max.y),
        ),
        label,
        TextStyle::new(font, surface.text).aligned(Align::Start),
        Align::Center,
    );
    response
}

pub fn text_button(ui: &mut Ui, id: &str, label: &str) -> bool {
    button(ui, id, label, Role::Quiet)
}

pub fn button_with_enabled(
    ui: &mut Ui,
    id: &str,
    label: &str,
    role: Role,
    width: Length,
    enabled: bool,
) -> Response {
    if enabled {
        return button_with(ui, id, label, role, width);
    }
    let metrics = ui.theme().metrics;
    let font = button_font(ui);
    let measured = ui.measure_text(label, TextStyle::new(font, Color::WHITE));
    let inset = padding(ui);
    let natural = measured.x + inset * 2.0;
    let width = match width {
        Length::Auto => natural,
        Length::Fill => ui.available().x,
        Length::Fixed(value) => value,
        Length::Minimum(minimum) => natural.max(minimum),
        Length::Maximum(maximum) => natural.min(maximum),
    };
    let rect = ui.allocate(Vec2::new(width, metrics.control_height));
    let surface = ui.surface_disabled(role);
    let mut painter = ui.painter();
    painter.outlined(
        rect,
        Corners::capsule(),
        surface.fill,
        surface.border,
        surface.shadow,
    );
    painter.text_in(
        rect,
        label,
        TextStyle::new(font, surface.text).aligned(Align::Center),
        Align::Center,
    );
    Response {
        rect,
        ..Response::default()
    }
}

pub fn icon_button(
    ui: &mut Ui,
    id: &str,
    symbol: Icon,
    size: f32,
    role: Role,
    tooltip: &str,
) -> bool {
    icon_button_response(ui, id, symbol, size, role, tooltip).clicked
}

pub fn icon_button_response(
    ui: &mut Ui,
    id: &str,
    symbol: Icon,
    size: f32,
    role: Role,
    tooltip: &str,
) -> Response {
    let key = ui.id(id);
    let rect = ui.allocate(Vec2::splat(size));
    let mut response = ui.interact(key, rect);
    if ui.focusable(key, rect, Corners::capsule()) && ui.activated(key) {
        response.clicked = true;
    }
    if response.hovered {
        ui.tooltip(tooltip);
    }
    let press = ui.animate(
        key ^ 0x71,
        if response.active { 1.0 } else { 0.0 },
        Motion::quick(),
    );
    let surface = ui.surface(key, role, &response);
    let body = rect.shrink(pressed_inset(press));
    let mut painter = ui.painter();
    painter.outlined(
        body,
        Corners::capsule(),
        surface.fill,
        surface.border,
        surface.shadow,
    );
    icon::paint(
        symbol,
        &mut painter,
        body.center(),
        (size * 0.50).clamp(11.0, 17.0),
        surface.text,
        1.6,
    );
    response
}

pub fn icon_button_enabled(
    ui: &mut Ui,
    id: &str,
    symbol: Icon,
    size: f32,
    role: Role,
    tooltip: &str,
    enabled: bool,
) -> bool {
    if !enabled {
        let rect = ui.allocate(Vec2::splat(size));
        let surface = ui.surface_disabled(role);
        let mut painter = ui.painter();
        painter.outlined(
            rect,
            surface.corners,
            surface.fill,
            surface.border,
            surface.shadow,
        );
        icon::paint(
            symbol,
            &mut painter,
            rect.center(),
            (size * 0.50).clamp(11.0, 17.0),
            surface.text,
            1.6,
        );
        return false;
    }
    icon_button_response(ui, id, symbol, size, role, tooltip).clicked
}

pub fn glass_icon_button(
    ui: &mut Ui,
    group: Option<GlassGroup>,
    id: &str,
    symbol: Icon,
    size: f32,
    style: GlassStyle,
    tooltip: &str,
) -> bool {
    glass_icon_button_response(ui, group, id, symbol, size, style, tooltip).clicked
}

pub fn glass_icon_button_response(
    ui: &mut Ui,
    group: Option<GlassGroup>,
    id: &str,
    symbol: Icon,
    size: f32,
    style: GlassStyle,
    tooltip: &str,
) -> Response {
    let key = ui.id(id);
    let rect = ui.allocate(Vec2::splat(size));
    let mut response = ui.interact(key, rect);
    if ui.focusable(key, rect, Corners::capsule()) && ui.activated(key) {
        response.clicked = true;
    }
    if response.hovered {
        ui.tooltip(tooltip);
    }
    ui.glass_in(group, key, rect, style.interactive());
    glass_ring(ui, key, rect, &response);
    let frame = ui.glass_frame(key).unwrap_or(rect);
    let color = ui.theme().palette.text_primary;
    let mut painter = ui.painter();
    icon::paint(
        symbol,
        &mut painter,
        frame.center(),
        (size * 0.50).clamp(11.0, 17.0),
        color,
        1.6,
    );
    response
}

pub fn glass_icon_button_disabled(
    ui: &mut Ui,
    group: Option<GlassGroup>,
    id: &str,
    symbol: Icon,
    size: f32,
    style: GlassStyle,
) -> Response {
    let key = ui.id(id);
    let rect = ui.allocate(Vec2::splat(size));
    ui.glass_in(group, key, rect, style);
    let frame = ui.glass_frame(key).unwrap_or(rect);
    let color = ui.theme().palette.text_quaternary;
    let mut painter = ui.painter();
    icon::paint(
        symbol,
        &mut painter,
        frame.center(),
        (size * 0.50).clamp(11.0, 17.0),
        color,
        1.6,
    );
    Response {
        rect,
        hovered: false,
        pressed: false,
        active: false,
        clicked: false,
        dragging: false,
    }
}

pub fn glass_menu_button(
    ui: &mut Ui,
    group: Option<GlassGroup>,
    id: &str,
    label: &str,
    dirty: bool,
    maximum: f32,
) -> Rect {
    let key = ui.id(id);
    let palette = ui.theme().palette;
    let font = ui.theme().font(crate::theme::Level::Callout).medium();
    let measured = ui.measure_text(label, TextStyle::new(font, Color::WHITE)).x;
    let label_width = measured.min(maximum);
    let width = label_width + if dirty { 30.0 } else { 26.0 };
    let rect = ui.allocate(Vec2::new(width, 34.0));
    let mut response = ui.interact(key, rect);
    if ui.focusable(key, rect, Corners::capsule()) && ui.activated(key) {
        response.clicked = true;
    }
    if response.clicked {
        ui.toggle_popup(key);
    }
    let active = ui.popup_open(key);
    let hover = ui.animate(
        key,
        if response.hovered || active { 1.0 } else { 0.0 },
        Motion::quick(),
    );
    ui.glass_in(group, key, rect, GlassStyle::button().interactive());
    glass_ring(ui, key, rect, &response);
    let frame = ui.glass_frame(key).unwrap_or(rect);
    let mut painter = ui.painter();
    painter.text_in(
        Rect::from_min_max(
            Vec2::new(frame.min.x + 11.0, frame.min.y),
            Vec2::new(frame.min.x + 11.0 + label_width, frame.max.y),
        ),
        label,
        TextStyle::new(
            font,
            palette.text_secondary.lerp(palette.text_primary, hover),
        )
        .truncating(),
        Align::Center,
    );
    if dirty {
        painter.rect(
            Rect::from_center_size(
                Vec2::new(frame.min.x + 16.0 + label_width, frame.center().y),
                Vec2::splat(5.0),
            ),
            Corners::capsule(),
            palette.warning,
        );
    }
    icon::paint(
        Icon::ChevronDown,
        &mut painter,
        Vec2::new(frame.max.x - 10.0, frame.center().y),
        9.0,
        palette.text_tertiary,
        1.5,
    );
    rect
}

pub fn glass_button(ui: &mut Ui, id: &str, label: &str, style: GlassStyle) -> Response {
    glass_button_core(ui, id, None, label, style)
}

pub fn glass_action_button(
    ui: &mut Ui,
    id: &str,
    symbol: Icon,
    label: &str,
    style: GlassStyle,
) -> Response {
    glass_button_core(ui, id, Some(symbol), label, style)
}

fn glass_button_core(
    ui: &mut Ui,
    id: &str,
    symbol: Option<Icon>,
    label: &str,
    style: GlassStyle,
) -> Response {
    let key = ui.id(id);
    let font = button_font(ui);
    let height = ui.theme().metrics.control_height + 8.0;
    let measured = ui.measure_text(label, TextStyle::new(font, Color::WHITE));
    let inset = padding(ui) + 2.0;
    let width = measured.x + inset * 2.0 + symbol.map_or(0.0, |_| 17.0);
    let rect = ui.allocate(Vec2::new(width, height));
    let mut response = ui.interact(key, rect);
    if ui.focusable(key, rect, Corners::capsule()) && ui.activated(key) {
        response.clicked = true;
    }
    ui.glass(key, rect, style.interactive());
    glass_ring(ui, key, rect, &response);
    let frame = ui.glass_frame(key).unwrap_or(rect);
    let color = ui.theme().palette.text_primary;
    let mut painter = ui.painter();
    let text = match symbol {
        Some(symbol) => {
            let glyph = Vec2::new(frame.min.x + inset + 3.0, frame.center().y);
            icon::paint(symbol, &mut painter, glyph, 14.0, color, 1.6);
            Rect::from_min_max(
                Vec2::new(glyph.x + 9.0, frame.min.y),
                Vec2::new(frame.max.x - inset, frame.max.y),
            )
        }
        None => frame.shrink2(Vec2::new(inset, 0.0)),
    };
    painter.text_in(
        text,
        label,
        TextStyle::new(font, color).aligned(Align::Center),
        Align::Center,
    );
    response
}

fn glass_ring(ui: &mut Ui, key: u64, rect: Rect, response: &Response) {
    let engaged = ui.animate(
        key ^ 0x91,
        if response.hovered || response.active {
            1.0
        } else {
            0.0
        },
        Motion::quick(),
    );
    if engaged <= 0.01 {
        return;
    }
    let palette = ui.theme().palette;
    let mut painter = ui.painter();
    painter.outlined(
        rect.shrink(0.5),
        Corners::capsule(),
        crate::style::Fill::None,
        Border {
            width: 1.0,
            color: palette.text_primary.amplify(0.16 * engaged),
        },
        crate::style::Shadow::NONE,
    );
}
