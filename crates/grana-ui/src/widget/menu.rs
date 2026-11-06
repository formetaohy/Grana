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
pub const SEPARATOR: f32 = 9.0;

pub fn menu_row<'a, A>(theme: &Theme, label: &str, checked: bool) -> Element<'a, A> {
    Element::new(Layout {
        size: Size::new(Length::Fill, Length::Fixed(MENU_ROW)),
        ..Layout::row()
    })
    .with_content(Content::MenuRow(MenuRowContent {
        label: label.to_string(),
        checked,
        font: theme.font(Level::Callout),
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
        font: theme.font(Level::Callout),
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
    if info.hover > 0.01 {
        painter.rect(
            info.rect.shrink2(Vec2::new(2.0, 0.0)),
            Corners::all(10.0),
            palette.selected.amplify(0.9 * info.hover),
        );
    }
    if row.checked {
        icon::paint(
            Icon::Check,
            painter,
            Vec2::new(info.rect.min.x + 12.0, info.rect.center().y),
            12.0,
            palette.text_primary,
            1.7,
        );
    }
    let text_color = if row.checked {
        palette.text_primary
    } else {
        palette
            .text_primary
            .lerp(palette.text_secondary, 1.0 - runtime.hover.value)
    };
    painter.text_in(
        Rect::from_min_max(
            Vec2::new(info.rect.min.x + 26.0, info.rect.min.y),
            info.rect.max,
        ),
        &row.label,
        TextStyle::new(row.font, text_color),
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
            Corners::all(10.0),
            palette.selected.amplify(0.9 * info.hover),
        );
    }
    let shortcut_width = row.shortcut.as_ref().map_or(8.0, |text| {
        18.0 + painter
            .measure(
                text,
                TextStyle::new(info.theme.font(Level::Caption), palette.text_tertiary),
            )
            .x
    });
    icon::paint(
        row.symbol,
        painter,
        Vec2::new(info.rect.min.x + 14.0, info.rect.center().y),
        14.0,
        palette
            .text_secondary
            .lerp(palette.text_primary, runtime.hover.value),
        1.5,
    );
    painter.text_in(
        Rect::from_min_max(
            Vec2::new(info.rect.min.x + 28.0, info.rect.min.y),
            Vec2::new(info.rect.max.x - shortcut_width, info.rect.max.y),
        ),
        &row.label,
        TextStyle::new(row.font, palette.text_primary),
        Align::Center,
    );
    if let Some(shortcut) = &row.shortcut {
        painter.text_in(
            info.rect.shrink2(Vec2::new(10.0, 0.0)),
            shortcut,
            TextStyle::new(info.theme.font(Level::Caption), palette.text_tertiary)
                .aligned(Align::End),
            Align::Center,
        );
    }
}

pub(super) fn paint_separator(painter: &mut Painter, info: &PaintInfo<'_>) {
    painter.rect(
        Rect::from_min_size(
            Vec2::new(info.rect.min.x + 10.0, info.rect.center().y),
            Vec2::new(info.rect.width() - 20.0, 1.0),
        ),
        Corners::ZERO,
        info.theme.palette.separator,
    );
}
