use glam::Vec2;
use grana_ui::{
    Align, BuildContext, Corners, Cursor, Edges, Element, Fill, Icon, IconStyle, Key, Layer,
    Length, Level, Rect, Role, State, Surface, TextStyle,
};

use crate::message::Message;
use crate::state::{SettingsTab, Workspace};

mod about;
mod general;
mod row;
mod update;

const WIDTH: f32 = 920.0;
const HEIGHT: f32 = 560.0;
const GUTTER: f32 = 24.0;
const PADDING: f32 = 32.0;
const HEADER_GAP: f32 = 24.0;
const CLOSE: f32 = 40.0;
const RAIL_WIDTH: f32 = 220.0;
const RAIL_ITEM: f32 = 48.0;
const RAIL_INSET: f32 = 14.0;
const RAIL_GAP: f32 = 10.0;
const RAIL_ICON: f32 = 20.0;
const RAIL_ITEM_GAP: f32 = 4.0;
const PANE_GAP: f32 = 28.0;
const PANE_TAIL: f32 = 8.0;
const SECTION_GAP: f32 = 24.0;
const SHEET_RADIUS: f32 = 28.0;

pub const WINDOW_SIZE: Vec2 = Vec2::new(WIDTH + 2.0 * GUTTER, HEIGHT + 2.0 * GUTTER);

pub fn view<'a>(
    ctx: &mut BuildContext<'_, Message>,
    full: Rect,
    workspace: &Workspace,
) -> Vec<Element<'a, Message>> {
    let open = workspace.settings.open;
    let size = Vec2::new(
        WIDTH.min((full.width() - 2.0 * GUTTER).max(1.0)),
        HEIGHT.min((full.height() - 2.0 * GUTTER).max(1.0)),
    );
    let Some(sheet) = crate::dialog::sheet(ctx, full, Key::new("settings.dialog"), open, size)
    else {
        return Vec::new();
    };
    let theme = *ctx.theme();
    let scrim = crate::dialog::scrim(
        ctx,
        full,
        "settings.scrim",
        open,
        Layer::Popup,
        Some(Message::SetSettings(None)),
    );
    let header = header(ctx, open);
    let body = grana_ui::row()
        .width(Length::Fill)
        .height(Length::Fill)
        .gap(PANE_GAP)
        .children([
            navigation(ctx, workspace, open),
            content(ctx, workspace, open),
        ]);
    let mut surface = theme.surface(Role::Dialog, State::Rest);
    surface.fill = Fill::Solid(theme.palette.surface_container_low);
    surface.corners = Corners::all(SHEET_RADIUS);
    let dialog = grana_ui::column()
        .at(sheet.rect)
        .named("settings.sheet")
        .background(surface)
        .padding(Edges::all(PADDING))
        .gap(HEADER_GAP)
        .clipped()
        .layer(Layer::Popup)
        .blocks_pointer()
        .modal()
        .children([header, body]);
    vec![scrim, dialog]
}

fn header<'a>(ctx: &mut BuildContext<'_, Message>, open: bool) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let title = ctx
        .text(
            grana_i18n::settings::title(ctx.language()),
            TextStyle::new(theme.font(Level::Display), theme.palette.on_surface),
        )
        .width(Length::Fill);
    let close = ctx
        .icon_button(
            Icon::Close,
            CLOSE,
            Role::SurfaceHigh,
            grana_i18n::settings::close(ctx.language()),
        )
        .named("settings.close");
    let close = if open {
        close.press(Message::SetSettings(None))
    } else {
        close
    };
    grana_ui::row()
        .width(Length::Fill)
        .height(Length::Fixed(CLOSE))
        .align(Align::Center)
        .children([title, close])
}

fn navigation<'a>(
    ctx: &mut BuildContext<'_, Message>,
    workspace: &Workspace,
    open: bool,
) -> Element<'a, Message> {
    let items = SettingsTab::ALL
        .into_iter()
        .enumerate()
        .map(|(index, tab)| nav_item(ctx, index, tab, tab == workspace.settings.tab, open))
        .collect::<Vec<_>>();
    grana_ui::column()
        .width(Length::Fixed(RAIL_WIDTH))
        .height(Length::Fill)
        .gap(RAIL_ITEM_GAP)
        .children(items)
}

fn nav_item<'a>(
    ctx: &mut BuildContext<'_, Message>,
    index: usize,
    tab: SettingsTab,
    selected: bool,
    open: bool,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let palette = theme.palette;
    let corners = Corners::capsule();
    let color = if selected {
        palette.on_secondary_container
    } else {
        palette.on_surface_variant
    };
    let base = if selected {
        Surface::fill(palette.secondary_container).with_corners(corners)
    } else {
        Surface::NONE
    };
    let hover = Surface::fill(if selected {
        palette
            .secondary_container
            .lerp(palette.on_secondary_container, 0.08)
    } else {
        palette.surface_container_high
    })
    .with_corners(corners);
    let item = grana_ui::row()
        .key(Key::index("settings.tab", index))
        .width(Length::Fill)
        .height(Length::Fixed(RAIL_ITEM))
        .padding(Edges::symmetric(RAIL_INSET, 0.0))
        .gap(RAIL_GAP)
        .align(Align::Center)
        .background(base)
        .hover_highlight(hover)
        .cursor(Cursor::Pointing)
        .focus_ring(corners)
        .children([
            ctx.icon(
                tab.symbol(),
                RAIL_ICON,
                color,
                if selected {
                    IconStyle::FILLED
                } else {
                    IconStyle::REGULAR
                },
            ),
            ctx.text(
                tab.name(ctx.language()),
                TextStyle::new(theme.font(Level::Title), color),
            ),
        ]);
    if open {
        item.press(Message::SetSettings(Some(tab)))
    } else {
        item
    }
}

fn content<'a>(
    ctx: &mut BuildContext<'_, Message>,
    workspace: &Workspace,
    open: bool,
) -> Element<'a, Message> {
    let sections = match workspace.settings.tab {
        SettingsTab::General => general::view(ctx, workspace, open),
        SettingsTab::Update => update::view(ctx, workspace, open),
        SettingsTab::About => about::view(ctx, workspace, open),
    };
    let scroll = match workspace.settings.tab {
        SettingsTab::General => "settings.scroll.general",
        SettingsTab::Update => "settings.scroll.update",
        SettingsTab::About => "settings.scroll.about",
    };
    grana_ui::column()
        .key(Key::new(scroll))
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(Edges {
            left: 0.0,
            top: 0.0,
            right: PANE_TAIL,
            bottom: 0.0,
        })
        .clipped()
        .scroll(grana_ui::Axis::Column, Length::Fill, None)
        .children([grana_ui::column()
            .width(Length::Fill)
            .gap(SECTION_GAP)
            .children(sections)])
}

pub(super) fn bytes(value: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = value as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}
