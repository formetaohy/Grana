use glam::Vec2;

use crate::content::{ActionRowContent, Content, MenuRowContent};
use crate::element::{Element, Layout, Length, Size};
use crate::geometry::{Corners, Rect};
use crate::icon::{self, Icon};
use crate::node::Runtime;
use crate::paint::Painter;
use crate::text::{Align, TextStyle};
use crate::theme::{Level, Theme};

use super::PaintInfo;

pub const MENU_ROW: f32 = 32.0;
pub const SEPARATOR: f32 = 8.0;

pub fn menu_row<'a, A>(theme: &Theme, label: &str, checked: bool) -> Element<'a, A> {
    Element::new(Layout {
        size: Size::new(Length::Fill, Length::Fixed(MENU_ROW)),
        ..Layout::row()
    })
    .with_content(Content::MenuRow(MenuRowContent {
        label: label.to_string(),
        checked,
        font: theme.font(Level::Label),
    }))
}

pub fn action_row<'a, A>(
    theme: &Theme,
    symbol: Icon,
    label: &str,
    shortcut: Option<&str>,
) -> Element<'a, A> {
    Element::new(Layout {
        size: Size::new(Length::Fill, Length::Fixed(MENU_ROW)),
        ..Layout::row()
    })
    .with_content(Content::ActionRow(ActionRowContent {
        symbol,
        label: label.to_string(),
        shortcut: shortcut.map(str::to_string),
        font: theme.font(Level::Label),
    }))
}

pub fn separator<'a, A>() -> Element<'a, A> {
    Element::new(Layout {
        size: Size::new(Length::Fill, Length::Fixed(SEPARATOR)),
        ..Layout::row()
    })
    .with_content(Content::Separator)
}

pub(super) fn paint_menu_row(
    row: &MenuRowContent,
    runtime: &Runtime,
    painter: &mut Painter,
    info: &PaintInfo<'_>,
) {
    let palette = info.theme.palette;
    let body = info.rect.shrink2(Vec2::new(2.0, 0.0));
    if row.checked {
        painter.rect(
            body,
            Corners::capsule(),
            palette
                .secondary_container
                .lerp(palette.on_secondary_container, 0.08 * info.hover),
        );
    } else if info.hover > 0.01 {
        painter.rect(
            body,
            Corners::all(info.theme.metrics.corner_sm),
            palette.on_surface.with_alpha(0.09 * info.hover),
        );
    }
    if row.checked {
        icon::paint(
            Icon::Check,
            painter,
            Vec2::new(info.rect.min.x + 14.0, info.rect.center().y),
            12.0,
            palette.on_secondary_container,
            1.8,
        );
    }
    let text_color = if row.checked {
        palette.on_secondary_container
    } else {
        palette
            .on_surface
            .lerp(palette.on_surface_variant, 1.0 - runtime.hover.value)
    };
    painter.text_in(
        Rect::from_min_max(
            Vec2::new(info.rect.min.x + 28.0, info.rect.min.y),
            Vec2::new(info.rect.max.x - 8.0, info.rect.max.y),
        ),
        &row.label,
        TextStyle::new(row.font, text_color).truncating(),
        Align::Center,
    );
}

pub(super) fn paint_action_row(
    row: &ActionRowContent,
    runtime: &Runtime,
    painter: &mut Painter,
    info: &PaintInfo<'_>,
) {
    let palette = info.theme.palette;
    if info.hover > 0.01 {
        painter.rect(
            info.rect.shrink2(Vec2::new(2.0, 0.0)),
            Corners::capsule(),
            palette.secondary_container.with_alpha(0.65 * info.hover),
        );
    }
    let shortcut_width = row.shortcut.as_ref().map_or(10.0, |text| {
        18.0 + painter
            .measure(
                text,
                TextStyle::new(info.theme.font(Level::Monospace), palette.outline),
            )
            .x
    });
    icon::paint(
        row.symbol,
        painter,
        Vec2::new(info.rect.min.x + 14.0, info.rect.center().y),
        13.5,
        palette
            .on_surface_variant
            .lerp(palette.on_secondary_container, runtime.hover.value),
        1.6,
    );
    painter.text_in(
        Rect::from_min_max(
            Vec2::new(info.rect.min.x + 29.0, info.rect.min.y),
            Vec2::new(info.rect.max.x - shortcut_width, info.rect.max.y),
        ),
        &row.label,
        TextStyle::new(
            row.font,
            palette
                .on_surface
                .lerp(palette.on_secondary_container, runtime.hover.value),
        )
        .truncating(),
        Align::Center,
    );
    if let Some(shortcut) = &row.shortcut {
        painter.text_in(
            info.rect.shrink2(Vec2::new(10.0, 0.0)),
            shortcut,
            TextStyle::new(info.theme.font(Level::Monospace), palette.outline).aligned(Align::End),
            Align::Center,
        );
    }
}

pub(super) fn paint_separator(painter: &mut Painter, info: &PaintInfo<'_>) {
    painter.rect(
        Rect::from_min_size(
            Vec2::new(info.rect.min.x + 8.0, info.rect.center().y),
            Vec2::new(info.rect.width() - 16.0, 1.0),
        ),
        Corners::ZERO,
        info.theme.palette.outline_variant.with_alpha(0.55),
    );
}
