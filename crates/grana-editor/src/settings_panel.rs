use grana_ui::{
    Align, BuildContext, Corners, Cursor, Edges, Element, Icon, IconStyle, Key, Layer, Length,
    Level, Rect, Role, Surface, TextStyle, Vec2,
};

use crate::message::{Message, UpdateAction};
use crate::state::{SettingsTab, Update, UpdateFailure, UpdatePhase, Workspace};

const WIDTH: f32 = 680.0;
const HEIGHT: f32 = 384.0;
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
const BAR_ROW: f32 = 20.0;
const LOGO: f32 = 40.0;
const DIVIDER: f32 = 1.0;

pub fn view<'a>(
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
        Some(Message::SetSettings(None)),
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
        close.press(Message::SetSettings(None))
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
    let pane = match workspace.settings.tab {
        SettingsTab::General => general(ctx, workspace.language, open),
        SettingsTab::About => about(ctx, workspace, open),
    };
    grana_ui::column()
        .width(Length::Fill)
        .height(Length::Fill)
        .clipped()
        .scroll(grana_ui::Axis::Column, Length::Fill, None)
        .children([pane])
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

fn action_row<'a>(
    ctx: &BuildContext<'_, Message>,
    children: Vec<Element<'a, Message>>,
) -> Element<'a, Message> {
    let palette = ctx.theme().palette;
    let corners = Corners::capsule();
    grana_ui::row()
        .width(Length::Fill)
        .height(Length::Fixed(ROW))
        .padding(Edges::symmetric(ROW_INSET, 0.0))
        .gap(ROW_GAP)
        .align(Align::Center)
        .hover_highlight(Surface::fill(palette.surface_container_highest).with_corners(corners))
        .cursor(Cursor::Pointing)
        .focus_ring(corners)
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
    let mut update = vec![update_action(ctx, workspace, open)];
    if workspace.update.notes.is_some() {
        update.push(note_row(ctx, open));
    }
    update.push(automatic_row(ctx, workspace, open));
    grana_ui::column()
        .width(Length::Fill)
        .gap(SECTION_TAIL)
        .children([
            section(ctx, grana_i18n::settings::about(language), vec![identity]),
            section(ctx, grana_i18n::settings::update(language), update),
        ])
}

fn update_action<'a>(
    ctx: &mut BuildContext<'_, Message>,
    workspace: &Workspace,
    open: bool,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let palette = theme.palette;
    let language = ctx.language();
    let update = &workspace.update;
    let (label, action) = match update.phase {
        UpdatePhase::Idle | UpdatePhase::Current | UpdatePhase::Failed(_) => {
            (grana_i18n::settings::check(language), UpdateAction::Check)
        }
        UpdatePhase::Checking | UpdatePhase::Downloading => {
            (grana_i18n::common::cancel(language), UpdateAction::Cancel)
        }
        UpdatePhase::Available => (
            grana_i18n::settings::download(language),
            UpdateAction::Download,
        ),
        UpdatePhase::Ready => (grana_i18n::settings::restart(language), UpdateAction::Apply),
    };
    let status_color = if matches!(update.phase, UpdatePhase::Failed(_)) {
        palette.error
    } else {
        palette.on_surface_variant
    };
    let mut lines = vec![
        ctx.text(
            label,
            TextStyle::new(theme.font(Level::Title), palette.on_surface),
        )
        .width(Length::Fill),
    ];
    if let Some(status) = status_text(update, language) {
        lines.push(ctx.text(
            status,
            TextStyle::new(theme.font(Level::Caption), status_color),
        ));
    }
    let item = action_row(
        ctx,
        vec![
            ctx.icon(
                Icon::Sparkles,
                ROW_ICON,
                palette.on_surface_variant,
                IconStyle::REGULAR,
            ),
            grana_ui::column()
                .width(Length::Fill)
                .gap(2.0)
                .children(lines),
        ],
    )
    .named("settings.update");
    let item = if open {
        item.press(Message::Update(action))
    } else {
        item
    };
    let mut body = vec![item];
    if let Some((done, total)) = update.progress {
        body.push(bar_row(ctx, done, total));
    }
    grana_ui::column().width(Length::Fill).children(body)
}

fn bar_row<'a>(ctx: &mut BuildContext<'_, Message>, done: u64, total: u64) -> Element<'a, Message> {
    let fraction = if total == 0 {
        0.0
    } else {
        done as f32 / total as f32
    };
    let width = WIDTH - 2.0 * PADDING - RAIL_WIDTH - PANE_GAP - 2.0 * ROW_INSET;
    grana_ui::row()
        .width(Length::Fill)
        .height(Length::Fixed(BAR_ROW))
        .padding(Edges::symmetric(ROW_INSET, 0.0))
        .children([ctx.progress(fraction.clamp(0.0, 1.0), width)])
}

fn note_row<'a>(ctx: &mut BuildContext<'_, Message>, open: bool) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let palette = theme.palette;
    let item = action_row(
        ctx,
        vec![
            ctx.icon(
                Icon::Globe,
                ROW_ICON,
                palette.on_surface_variant,
                IconStyle::REGULAR,
            ),
            ctx.text(
                grana_i18n::settings::notes(ctx.language()),
                TextStyle::new(theme.font(Level::Title), palette.on_surface),
            )
            .width(Length::Fill),
            ctx.icon(
                Icon::ChevronRight,
                RAIL_ICON,
                palette.on_surface_variant,
                IconStyle::REGULAR,
            ),
        ],
    )
    .named("settings.notes");
    if open {
        item.press(Message::Update(UpdateAction::Notes))
    } else {
        item
    }
}

fn automatic_row<'a>(
    ctx: &mut BuildContext<'_, Message>,
    workspace: &Workspace,
    open: bool,
) -> Element<'a, Message> {
    let language = ctx.language();
    let toggle = ctx
        .toggle_row(
            grana_i18n::settings::automatic(language),
            workspace.update.automatic,
        )
        .named("settings.automatic")
        .enabled(open)
        .on_toggle(|_| Message::Update(UpdateAction::ToggleCheck));
    grana_ui::row()
        .width(Length::Fill)
        .padding(Edges::symmetric(ROW_INSET, 0.0))
        .children([toggle])
}

fn status_text(update: &Update, language: grana_i18n::Language) -> Option<String> {
    match update.phase {
        UpdatePhase::Idle => None,
        UpdatePhase::Checking => Some(grana_i18n::settings::checking(language).to_string()),
        UpdatePhase::Current => Some(grana_i18n::settings::up_to_date(language).to_string()),
        UpdatePhase::Available => update.version.clone().map(|version| match update.size {
            Some(size) => format!("{version} · {}", size_text(size)),
            None => version,
        }),
        UpdatePhase::Downloading => update
            .progress
            .map(|(done, total)| download_text(done, total, language)),
        UpdatePhase::Ready => Some(grana_i18n::settings::ready(language).to_string()),
        UpdatePhase::Failed(failure) => Some(failure_text(failure, language).to_string()),
    }
}

fn download_text(done: u64, total: u64, language: grana_i18n::Language) -> String {
    if total == 0 {
        grana_i18n::settings::downloading(language).to_string()
    } else {
        format!(
            "{:.0}% · {:.1} MB",
            done as f32 / total as f32 * 100.0,
            total as f64 / (1024.0 * 1024.0)
        )
    }
}

fn failure_text(failure: UpdateFailure, language: grana_i18n::Language) -> &'static str {
    match failure {
        UpdateFailure::Offline => grana_i18n::settings::offline(language),
        UpdateFailure::Corrupt => grana_i18n::settings::corrupt(language),
        UpdateFailure::Signature => grana_i18n::settings::signature(language),
        UpdateFailure::Storage => grana_i18n::settings::storage(language),
        UpdateFailure::Installer => grana_i18n::settings::installer(language),
        UpdateFailure::Other => grana_i18n::settings::failure(language),
    }
}

fn size_text(size: u64) -> String {
    if size >= 1 << 20 {
        format!("{:.1} MB", size as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.0} KB", size as f64 / 1024.0)
    }
}
