use grana_ui::{
    Align, BuildContext, Corners, Cursor, Edges, Element, Icon, IconStyle, Key, Layer, Length,
    Level, Rect, Role, Surface, TextStyle, Vec2,
};

use crate::message::{Message, UpdateAction};
use crate::state::{SettingsTab, Workspace};

const WIDTH: f32 = 680.0;
const HEIGHT: f32 = 320.0;
const GUTTER: f32 = 24.0;
const PADDING: f32 = 24.0;
const HEADER_GAP: f32 = 16.0;
const CLOSE: f32 = 40.0;
const RAIL_WIDTH: f32 = 168.0;
const RAIL_ITEM: f32 = 44.0;
const RAIL_INSET: f32 = 14.0;
const RAIL_GAP: f32 = 10.0;
const RAIL_ICON: f32 = 18.0;
const PANE_GAP: f32 = 28.0;
const SECTION_GAP: f32 = 8.0;
const SECTION_TAIL: f32 = 20.0;
const ROW: f32 = 56.0;
const ROW_INSET: f32 = 12.0;
const ROW_GAP: f32 = 12.0;
const ROW_ICON: f32 = 20.0;
const LOGO: f32 = 40.0;
const DIVIDER: f32 = 1.0;

pub(crate) fn view<'a>(
    ctx: &mut BuildContext<'_, Message>,
    full: Rect,
    workspace: &Workspace,
) -> Vec<Element<'a, Message>> {
    let open = workspace.settings.open;
    let size = Vec2::new(
        WIDTH.min((full.width() - 2.0 * GUTTER).max(520.0)),
        HEIGHT.min((full.height() - 2.0 * GUTTER).max(280.0)),
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
        Some(Message::ToggleSettings(false)),
    );
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
            Role::Standard,
            grana_i18n::settings::close(ctx.language()),
        )
        .named("settings.close");
    let close = if open {
        close.press(Message::ToggleSettings(false))
    } else {
        close
    };
    let header = grana_ui::row()
        .width(Length::Fill)
        .align(Align::Center)
        .children([title, close]);
    let body = grana_ui::row()
        .width(Length::Fill)
        .height(Length::Fill)
        .gap(PANE_GAP)
        .children([
            navigation(ctx, workspace, open),
            content(ctx, workspace, open),
        ]);
    let dialog = grana_ui::column()
        .at(sheet.rect)
        .named("settings.sheet")
        .background(sheet.surface)
        .padding(Edges::all(PADDING))
        .gap(HEADER_GAP)
        .clipped()
        .layer(Layer::Popup)
        .blocks_pointer()
        .modal()
        .children([header, body]);
    vec![scrim, dialog]
}

fn navigation<'a>(
    ctx: &mut BuildContext<'_, Message>,
    workspace: &Workspace,
    open: bool,
) -> Element<'a, Message> {
    let gap = ctx.theme().metrics.group_gap;
    let items = SettingsTab::ALL
        .into_iter()
        .enumerate()
        .map(|(index, tab)| nav_item(ctx, index, tab, tab == workspace.settings.tab, open))
        .collect::<Vec<_>>();
    grana_ui::column()
        .width(Length::Fixed(RAIL_WIDTH))
        .height(Length::Fill)
        .gap(gap)
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
        palette.surface_container_highest
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
            ctx.icon(tab.symbol(), RAIL_ICON, color, IconStyle::REGULAR),
            ctx.text(
                tab.name(ctx.language()),
                TextStyle::new(theme.font(Level::Title), color),
            ),
        ]);
    if open {
        item.press(Message::SetSettingsTab(tab))
    } else {
        item
    }
}

fn content<'a>(
    ctx: &mut BuildContext<'_, Message>,
    workspace: &Workspace,
    open: bool,
) -> Element<'a, Message> {
    match workspace.settings.tab {
        SettingsTab::General => general(ctx, workspace.language, open),
        SettingsTab::About => about(ctx, workspace, open),
    }
}

fn section<'a>(
    ctx: &mut BuildContext<'_, Message>,
    title: &str,
    rows: Vec<Element<'a, Message>>,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let label = ctx.text(
        title.to_uppercase(),
        TextStyle::new(
            theme.font(Level::Overline),
            theme.palette.on_surface_variant,
        ),
    );
    let head = grana_ui::row()
        .width(Length::Fill)
        .padding(Edges {
            left: ROW_INSET,
            top: 0.0,
            right: 0.0,
            bottom: 0.0,
        })
        .children([label]);
    let mut list = Vec::new();
    for (index, row) in rows.into_iter().enumerate() {
        if index > 0 {
            list.push(divider(ctx));
        }
        list.push(row);
    }
    grana_ui::column()
        .width(Length::Fill)
        .gap(SECTION_GAP)
        .children([head, grana_ui::column().width(Length::Fill).children(list)])
}

fn divider<'a>(ctx: &BuildContext<'_, Message>) -> Element<'a, Message> {
    grana_ui::column()
        .width(Length::Fill)
        .height(Length::Fixed(DIVIDER))
        .background(Surface::fill(ctx.theme().palette.outline_variant))
}

fn row<'a>(children: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    grana_ui::row()
        .width(Length::Fill)
        .height(Length::Fixed(ROW))
        .padding(Edges::symmetric(ROW_INSET, 0.0))
        .gap(ROW_GAP)
        .align(Align::Center)
        .children(children)
}

fn general<'a>(
    ctx: &mut BuildContext<'_, Message>,
    language: grana_i18n::Language,
    open: bool,
) -> Element<'a, Message> {
    let labels = grana_i18n::Language::ALL.map(|candidate| candidate.label());
    let selected = grana_i18n::Language::ALL
        .iter()
        .position(|candidate| *candidate == language)
        .unwrap_or(0);
    let selector =
        crate::view::segmented_fill(ctx, "settings.language", &labels, selected, |index| {
            Message::SetLanguage(
                grana_i18n::Language::ALL
                    .get(index)
                    .copied()
                    .unwrap_or(grana_i18n::Language::English),
            )
        })
        .enabled(open);
    let title = grana_i18n::settings::language(ctx.language());
    let control = row(vec![selector]);
    section(ctx, title, vec![control])
}

fn about<'a>(
    ctx: &mut BuildContext<'_, Message>,
    workspace: &Workspace,
    open: bool,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let palette = theme.palette;
    let language = ctx.language();
    let mark = grana_ui::canvas(Length::Fixed(LOGO), LOGO)
        .named("settings.brand")
        .view_static(|painter, info| grana_brand::paint(painter, info.rect));
    let name = ctx
        .text(
            "Grana",
            TextStyle::new(theme.font(Level::Title), palette.on_surface),
        )
        .width(Length::Fill);
    let version = ctx.text(
        format!(
            "{} {}",
            grana_i18n::settings::version(language),
            env!("CARGO_PKG_VERSION")
        ),
        TextStyle::new(theme.font(Level::Caption), palette.on_surface_variant),
    );
    let identity = row(vec![
        mark,
        grana_ui::column()
            .width(Length::Fill)
            .gap(2.0)
            .children([name, version]),
    ]);
    let label = if workspace.update.phase.noticed() {
        grana_i18n::settings::open(language)
    } else {
        grana_i18n::settings::check(language)
    };
    let action = update_row(ctx, label, open);
    grana_ui::column()
        .width(Length::Fill)
        .height(Length::Fill)
        .gap(SECTION_TAIL)
        .children([
            section(ctx, grana_i18n::settings::about(language), vec![identity]),
            section(ctx, grana_i18n::settings::update(language), vec![action]),
        ])
}

fn update_row<'a>(
    ctx: &mut BuildContext<'_, Message>,
    label: &str,
    open: bool,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let palette = theme.palette;
    let corners = Corners::capsule();
    let item = grana_ui::row()
        .width(Length::Fill)
        .height(Length::Fixed(ROW))
        .padding(Edges::symmetric(ROW_INSET, 0.0))
        .gap(ROW_GAP)
        .align(Align::Center)
        .hover_highlight(Surface::fill(palette.surface_container_highest).with_corners(corners))
        .cursor(Cursor::Pointing)
        .focus_ring(corners)
        .children([
            ctx.icon(
                Icon::Sparkles,
                ROW_ICON,
                palette.on_surface_variant,
                IconStyle::REGULAR,
            ),
            ctx.text(
                label,
                TextStyle::new(theme.font(Level::Title), palette.on_surface),
            )
            .width(Length::Fill),
            ctx.icon(
                Icon::ChevronRight,
                RAIL_ICON,
                palette.on_surface_variant,
                IconStyle::REGULAR,
            ),
        ])
        .named("settings.update");
    if open {
        item.press(Message::Update(UpdateAction::Open))
    } else {
        item
    }
}
