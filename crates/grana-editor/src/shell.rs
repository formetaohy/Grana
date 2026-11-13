use glam::Vec2;
use grana_session::{Command, Kind, RecentEntry, RecentKind, RecoveryView, SessionView};
use grana_ui::{
    Align, BuildContext, Corners, Edges, Element, Fill, Icon, Key, Layer, Length, Level, Rect,
    Role, Shadow, State, TextStyle,
};

use crate::message::Message;
use crate::state::Workspace;

const CARD_WIDTH: f32 = 472.0;
const CARD_PADDING: f32 = 24.0;
const CARD_GAP: f32 = 14.0;
const HERO_HEIGHT: f32 = 52.0;
const RECENT_HEADER: f32 = 22.0;
const RECENT_ROW: f32 = 42.0;
const RECOVERY_HEIGHT: f32 = 88.0;
const STITCH_ROW: f32 = 30.0;
const STITCH_GAP: f32 = 8.0;

struct Status {
    text: String,
    detail: String,
    busy: bool,
    error: bool,
}

pub(crate) fn view<'a>(
    ctx: &mut BuildContext<'_, Message>,
    view: &SessionView<'_>,
    workspace: &Workspace,
    full: Rect,
) -> Vec<Element<'a, Message>> {
    let theme = *ctx.theme();
    let palette = theme.palette;
    let backdrop = palette.surface_dim;
    let status = status(view, workspace.language);
    let language = workspace.language;
    let detail_style = TextStyle::new(
        theme.font(Level::Body),
        if status.error {
            palette.error
        } else {
            palette.on_surface_variant
        },
    )
    .wrapping();
    let detail = ctx.layout_text(
        &status.detail,
        detail_style,
        CARD_WIDTH - 2.0 * CARD_PADDING,
    );
    let detail_height = detail.size.y.max(18.0);
    let button = theme.metrics.control_height;
    let shown = view.recent.len().min(crate::RECENT_LIMIT);
    let recents = if shown > 0 {
        CARD_GAP
            + RECENT_HEADER
            + RECENT_ROW * shown as f32
            + theme.metrics.group_gap * (shown as f32 - 1.0)
    } else {
        0.0
    };
    let recovered = if view.recovery.is_some() {
        CARD_GAP + RECOVERY_HEIGHT
    } else {
        0.0
    };
    let actions_height = match view.stitch.as_ref() {
        Some(stitch) => {
            let rows =
                3 + usize::from(stitch.channels.len() > 1) + usize::from(stitch.series_count > 1);
            STITCH_ROW * rows as f32 + STITCH_GAP * (rows as f32 - 1.0) + CARD_GAP
        }
        None => 38.0 + 8.0 + button + CARD_GAP,
    };
    let height = 2.0 * CARD_PADDING
        + HERO_HEIGHT
        + CARD_GAP
        + detail_height
        + CARD_GAP
        + actions_height
        + recovered
        + recents;
    let rect = Rect::from_min_size(
        Vec2::new(
            full.center().x - 0.5 * CARD_WIDTH,
            full.center().y - 0.52 * height,
        ),
        Vec2::new(CARD_WIDTH, height),
    );
    let glow = Rect::from_min_size(full.min, Vec2::new(full.width(), 0.58 * full.height()));
    let primary = palette.primary;
    let mut elements = vec![
        grana_ui::canvas(Length::Fill, full.height())
            .paint(move |painter, info| {
                painter.rect(info.rect, Corners::ZERO, backdrop);
                painter.gradient(glow, Corners::ZERO, backdrop.lerp(primary, 0.09), backdrop);
            })
            .named("shell.backdrop")
            .at(full)
            .layer(Layer::Content),
    ];
    let mut children = vec![
        hero_header(ctx, &status),
        grana_ui::text(status.detail.clone(), detail_style),
    ];
    let mut popups = Vec::new();
    match view.stitch.as_ref() {
        Some(stitch) => children.push(stitch_block(ctx, stitch, &mut popups)),
        None => {
            children.push(primary_actions(ctx, &mut popups));
            if let Some(recovery) = view.recovery.as_ref() {
                children.push(recovery_block(ctx, recovery));
            }
            let inner = theme.metrics.corner_sm;
            let third = (CARD_WIDTH - 2.0 * CARD_PADDING - 2.0 * theme.metrics.group_gap) / 3.0;
            children.push(
                grana_ui::row().gap(theme.metrics.group_gap).children([
                    grana_ui::widget::action_button(
                        &theme,
                        Icon::Folder,
                        grana_i18n::shell::ome_zarr_folder(language),
                        Role::SurfaceHigh,
                    )
                    .shaped(Corners::group_horizontal(
                        0,
                        3,
                        Corners::CAPSULE,
                        inner,
                        false,
                    ))
                    .width(Length::Fixed(third))
                    .named("shell.open.dataset")
                    .press(Message::Command(Command::OpenDataset)),
                    grana_ui::widget::action_button(
                        &theme,
                        Icon::Globe,
                        grana_i18n::shell::remote_ome_zarr(language),
                        Role::SurfaceHigh,
                    )
                    .shaped(Corners::group_horizontal(
                        1,
                        3,
                        Corners::CAPSULE,
                        inner,
                        false,
                    ))
                    .width(Length::Fixed(third))
                    .named("shell.open.remote")
                    .press(Message::BeginRemote),
                    grana_ui::widget::action_button(
                        &theme,
                        Icon::Folder,
                        grana_i18n::shell::project(language),
                        Role::SurfaceHigh,
                    )
                    .shaped(Corners::group_horizontal(
                        2,
                        3,
                        Corners::CAPSULE,
                        inner,
                        false,
                    ))
                    .width(Length::Fixed(third))
                    .named("shell.open.project")
                    .press(Message::Command(Command::OpenProject)),
                ]),
            );
        }
    }
    if shown > 0 {
        children.push(recent_block(ctx, view));
    }
    let surface = theme
        .surface(Role::Sheet, State::Rest)
        .with_corners(Corners::all(theme.metrics.corner_xxl))
        .with_shadow(Shadow::new(grana_ui::Color::black_alpha(0.58), 32.0, 10.0));
    let card = grana_ui::column()
        .padding(Edges::all(CARD_PADDING))
        .gap(CARD_GAP)
        .background(surface)
        .children(children)
        .at(rect)
        .named("shell.card");
    elements.push(card);
    elements.extend(popups);
    elements.extend(crate::panel::remote(ctx, full, workspace.remote_open));
    elements
}

fn hero_header<'a>(ctx: &mut BuildContext<'_, Message>, status: &Status) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let palette = theme.palette;
    let brand = grana_ui::canvas(Length::Fixed(48.0), 48.0)
        .paint(|painter, info| {
            let badge = Rect::from_center_size(info.rect.center(), Vec2::splat(44.0));
            grana_brand::paint(painter, badge);
        })
        .named("shell.brand");
    let title_color = if status.error {
        palette.error
    } else {
        palette.on_surface
    };
    let mut status_row = Vec::new();
    if status.busy {
        status_row.push(crate::view::spinner(ctx, 16.0));
    }
    status_row.push(grana_ui::text(
        status.text.clone(),
        TextStyle::new(theme.font(Level::Headline), title_color).truncating(),
    ));
    let title_col = grana_ui::column()
        .width(Length::Fixed(
            CARD_WIDTH - 2.0 * CARD_PADDING - 48.0 - 124.0 - 24.0,
        ))
        .gap(2.0)
        .justify(Align::Center)
        .children([
            grana_ui::text(
                "Grana",
                TextStyle::new(theme.font(Level::Display), palette.on_surface),
            ),
            grana_ui::row()
                .gap(6.0)
                .align(Align::Center)
                .children(status_row),
        ]);
    let language = ctx.language();
    let labels = grana_i18n::Language::ALL.map(grana_i18n::Language::label);
    let selected = grana_i18n::Language::ALL
        .iter()
        .position(|candidate| *candidate == language)
        .unwrap_or(0);
    let lang_switch = crate::view::segmented(ctx, "shell.language", &labels, selected, |index| {
        match grana_i18n::Language::ALL.get(index) {
            Some(language) => Message::SetLanguage(*language),
            None => Message::Noop,
        }
    });
    grana_ui::row()
        .gap(12.0)
        .align(Align::Center)
        .height(Length::Fixed(HERO_HEIGHT))
        .children([brand, title_col, lang_switch])
}

fn primary_actions<'a>(
    ctx: &mut BuildContext<'_, Message>,
    popups: &mut Vec<Element<'a, Message>>,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let language = ctx.language();
    let inner = theme.metrics.corner_xs;
    let menu_key = Key::new("shell.open.menu");
    let open = ctx.popup_open(menu_key);
    let lead_w = CARD_WIDTH - 2.0 * CARD_PADDING - 38.0 - theme.metrics.group_gap;
    let lead = grana_ui::widget::action_button(
        &theme,
        Icon::Cube,
        grana_i18n::shell::open_image(language),
        Role::Filled,
    )
    .shaped(Corners::split_leading(inner))
    .width(Length::Fixed(lead_w))
    .height(Length::Fixed(38.0))
    .named("shell.open.image")
    .press(Message::Command(Command::OpenImage));
    let trail = grana_ui::widget::icon_button(
        if open {
            Icon::ChevronUp
        } else {
            Icon::ChevronDown
        },
        38.0,
        Role::Filled,
        "",
    )
    .shaped(if open {
        Corners::capsule()
    } else {
        Corners::split_trailing(inner)
    })
    .key(menu_key)
    .press(Message::Noop);
    if open {
        let rows = vec![
            crate::view::action_menu_message(
                ctx,
                "shell.menu.image",
                Icon::Cube,
                crate::text::command(language, "open.image"),
                None,
                Message::Command(Command::OpenImage),
            ),
            crate::view::action_menu_message(
                ctx,
                "shell.menu.tiles",
                Icon::Grid,
                crate::text::command(language, "open.tiles"),
                None,
                Message::Command(Command::OpenTiles),
            ),
            crate::view::action_menu_message(
                ctx,
                "shell.menu.folder",
                Icon::Folder,
                crate::text::command(language, "open.folder"),
                None,
                Message::Command(Command::OpenDataset),
            ),
            crate::view::action_menu_message(
                ctx,
                "shell.menu.remote",
                Icon::Globe,
                crate::text::command(language, "open.remote"),
                None,
                Message::BeginRemote,
            ),
            crate::view::menu_separator(ctx),
            crate::view::action_menu_message(
                ctx,
                "shell.menu.project",
                Icon::Folder,
                crate::text::command(language, "open.project"),
                None,
                Message::Command(Command::OpenProject),
            ),
            crate::view::action_menu_message(
                ctx,
                "shell.menu.package",
                Icon::Export,
                grana_i18n::shell::package(language),
                None,
                Message::Command(Command::OpenPackage),
            ),
        ];
        popups.push(crate::view::menu(ctx, menu_key, 240.0, rows));
    } else {
        popups.push(grana_ui::popover::popup(
            menu_key,
            240.0,
            std::iter::empty(),
        ));
    }
    grana_ui::row()
        .gap(theme.metrics.group_gap)
        .align(Align::Center)
        .height(Length::Fixed(38.0))
        .children([lead, trail])
}

fn stitch_block<'a>(
    ctx: &mut BuildContext<'_, Message>,
    stitch: &grana_session::StitchView,
    popups: &mut Vec<Element<'a, Message>>,
) -> Element<'a, Message> {
    let language = ctx.language();
    let grid_key = Key::new("shell.stitch.grid");
    let channel_key = Key::new("shell.stitch.channel");
    let detail = if stitch.running {
        grana_i18n::shell::stitching(language, stitch.tiles)
    } else {
        grana_i18n::shell::tile_count(language, stitch.tiles)
    };
    let mut children = vec![crate::view::card_title(
        ctx,
        Icon::Grid,
        grana_i18n::shell::tiles(language),
        Some(&detail),
    )];
    let rows = grids(stitch.tiles);
    if rows.len() > 1 {
        let label = format!("{} × {}", stitch.rows, stitch.columns);
        let button = crate::view::button_with(ctx, &label, Role::SurfaceHigh, Length::Fill)
            .key(grid_key)
            .press(Message::ToggleOpen(grid_key));
        children.push(crate::view::form_row(
            ctx,
            grana_i18n::shell::grid(language),
            button,
        ));
        let entries = rows
            .into_iter()
            .map(|(rows, columns)| {
                let label = format!("{rows} × {columns}");
                let checked = rows == stitch.rows && columns == stitch.columns;
                crate::view::menu_row(
                    ctx,
                    &format!("shell.stitch.grid.{rows}.{columns}"),
                    &label,
                    checked,
                    Some(Message::Command(Command::SetStitchGrid { rows, columns })),
                )
            })
            .collect::<Vec<_>>();
        popups.push(crate::view::menu(ctx, grid_key, 140.0, entries));
    }
    if stitch.channels.len() > 1 {
        let label = stitch
            .channels
            .get(stitch.channel)
            .cloned()
            .unwrap_or_default();
        let button = crate::view::button_with(ctx, &label, Role::SurfaceHigh, Length::Fill)
            .key(channel_key)
            .press(Message::ToggleOpen(channel_key));
        children.push(crate::view::form_row(
            ctx,
            grana_i18n::shell::channel(language),
            button,
        ));
        let entries = stitch
            .channels
            .iter()
            .enumerate()
            .map(|(index, name)| {
                crate::view::menu_row(
                    ctx,
                    &format!("shell.stitch.channel.{index}"),
                    name,
                    index == stitch.channel,
                    Some(Message::Command(Command::SetStitchChannel(index))),
                )
            })
            .collect::<Vec<_>>();
        popups.push(crate::view::menu(ctx, channel_key, 176.0, entries));
    }
    if stitch.series_count > 1 {
        let series_key = Key::new("shell.stitch.series");
        let label = format!("{} / {}", stitch.series + 1, stitch.series_count);
        let button = crate::view::button_with(ctx, &label, Role::SurfaceHigh, Length::Fill)
            .key(series_key)
            .press(Message::ToggleOpen(series_key));
        children.push(crate::view::form_row(
            ctx,
            grana_i18n::shell::series(language),
            button,
        ));
        let entries = (0..stitch.series_count)
            .map(|index| {
                let label = format!("{} / {}", index + 1, stitch.series_count);
                crate::view::menu_row(
                    ctx,
                    &format!("shell.stitch.series.{index}"),
                    &label,
                    index == stitch.series,
                    Some(Message::Command(Command::SetStitchSeries(index))),
                )
            })
            .collect::<Vec<_>>();
        popups.push(crate::view::menu(ctx, series_key, 140.0, entries));
    }
    let confirm = if stitch.running {
        Message::Noop
    } else {
        Message::Command(Command::ConfirmStitch)
    };
    let label = if stitch.running {
        grana_i18n::shell::stitching(language, stitch.tiles)
    } else {
        grana_i18n::shell::stitch(language).to_string()
    };
    let mut row = Vec::new();
    if stitch.running {
        row.push(crate::view::spinner(ctx, 16.0));
    }
    row.push(
        crate::view::button_with(ctx, &label, Role::Filled, Length::Fill)
            .named("shell.stitch.confirm")
            .press(confirm),
    );
    row.push(crate::view::icon_button(
        ctx,
        "shell.stitch.close",
        Icon::Close,
        30.0,
        Role::SurfaceHigh,
        "",
        Message::Command(Command::CancelStitch),
    ));
    children.push(grana_ui::row().gap(6.0).align(Align::Center).children(row));
    crate::view::card(ctx, STITCH_GAP, children)
}

fn grids(tiles: usize) -> Vec<(usize, usize)> {
    let mut rows = Vec::new();
    for row in 1..=tiles {
        if tiles.is_multiple_of(row) {
            rows.push((row, tiles / row));
        }
    }
    rows
}

fn status(view: &SessionView<'_>, language: grana_i18n::Language) -> Status {
    if let Some(stitch) = view.stitch.as_ref() {
        return Status {
            text: grana_i18n::shell::no_dataset(language).to_string(),
            detail: stitch.folder.clone(),
            busy: stitch.running,
            error: false,
        };
    }
    if let Some((label, done, total)) = view.io.document {
        return Status {
            text: grana_i18n::common::task(language, label),
            detail: grana_i18n::shell::steps_complete(language, done, total),
            busy: true,
            error: false,
        };
    }
    if let Some(label) = view.io.loading {
        return Status {
            text: grana_i18n::shell::opening(language, label),
            detail: grana_i18n::shell::loading_detail(language).to_string(),
            busy: true,
            error: false,
        };
    }
    if let Some((label, done, total)) = view.io.label {
        return Status {
            text: grana_i18n::common::task(language, label),
            detail: grana_i18n::shell::steps_stored(language, done, total),
            busy: true,
            error: false,
        };
    }
    if let Some(message) = view.io.message {
        return Status {
            text: match message.kind() {
                Kind::Error => grana_i18n::shell::nothing_opened(language).to_string(),
                Kind::Info => grana_i18n::shell::ready(language).to_string(),
            },
            detail: message.text().to_string(),
            busy: false,
            error: message.kind() == Kind::Error,
        };
    }
    Status {
        text: grana_i18n::shell::no_dataset(language).to_string(),
        detail: grana_i18n::shell::no_dataset_detail(language).to_string(),
        busy: false,
        error: false,
    }
}

fn recovery_block<'a>(
    ctx: &mut BuildContext<'_, Message>,
    recovery: &RecoveryView<'_>,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let palette = theme.palette;
    let language = ctx.language();
    let headline = grana_i18n::shell::unsaved_analysis(language, recovery.name);
    let saved = ago(language, recovery.age);
    let detail = grana_i18n::shell::saved_detail(language, &recovery.detail, &saved);
    let mut surface = theme.surface(Role::Card, State::Rest);
    surface.fill = Fill::Solid(palette.tertiary_container);
    grana_ui::column()
        .padding(Edges::all(12.0))
        .gap(6.0)
        .height(Length::Fixed(RECOVERY_HEIGHT))
        .background(surface)
        .children([
            grana_ui::text(
                headline,
                TextStyle::new(theme.font(Level::Title), palette.on_tertiary_container)
                    .truncating(),
            ),
            grana_ui::text(
                detail,
                TextStyle::new(
                    theme.font(Level::Caption),
                    palette.on_tertiary_container.with_alpha(0.82),
                )
                .truncating(),
            ),
            grana_ui::row().gap(6.0).children([
                grana_ui::widget::action_button(
                    &theme,
                    Icon::Undo,
                    grana_i18n::shell::recover(language),
                    Role::Filled,
                )
                .named("shell.recover")
                .press(Message::Command(Command::RecoverAnalysis)),
                grana_ui::widget::action_button(
                    &theme,
                    Icon::Trash,
                    grana_i18n::shell::discard(language),
                    Role::SurfaceHigh,
                )
                .named("shell.discard")
                .press(Message::Command(Command::DiscardAnalysis)),
            ]),
        ])
}

fn ago(language: grana_i18n::Language, seconds: u64) -> String {
    match seconds {
        0..=59 => grana_i18n::shell::moments_ago(language).to_string(),
        60..=119 => grana_i18n::shell::minute_ago(language).to_string(),
        120..=3599 => grana_i18n::shell::minutes_ago(language, seconds / 60),
        3600..=7199 => grana_i18n::shell::hour_ago(language).to_string(),
        7200..=86399 => grana_i18n::shell::hours_ago(language, seconds / 3600),
        86400..=172799 => grana_i18n::shell::day_ago(language).to_string(),
        _ => grana_i18n::shell::days_ago(language, seconds / 86400),
    }
}

fn recent_block<'a>(
    ctx: &mut BuildContext<'_, Message>,
    view: &SessionView<'_>,
) -> Element<'a, Message> {
    let entries = &view.recent[..view.recent.len().min(crate::RECENT_LIMIT)];
    if entries.is_empty() {
        return grana_ui::column().height(Length::Fixed(0.0));
    }
    let theme = *ctx.theme();
    let palette = theme.palette;
    let header_style = TextStyle::new(theme.font(Level::Overline), palette.on_surface_variant);
    let recent_label = grana_i18n::shell::recent(ctx.language());
    let mut children = vec![
        grana_ui::canvas(Length::Fill, RECENT_HEADER)
            .paint(move |painter, info| {
                painter.text_in(
                    Rect::from_min_max(
                        Vec2::new(info.rect.min.x + 4.0, info.rect.min.y),
                        info.rect.max,
                    ),
                    recent_label,
                    header_style,
                    Align::Center,
                );
            })
            .named("shell.recent.header"),
    ];
    let count = entries.len();
    for (index, entry) in entries.iter().enumerate() {
        children.push(recent_row(ctx, index, count, entry));
    }
    grana_ui::column()
        .gap(theme.metrics.group_gap)
        .children(children)
}

fn recent_row<'a>(
    ctx: &mut BuildContext<'_, Message>,
    index: usize,
    count: usize,
    entry: &RecentEntry,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let palette = theme.palette;
    let corners = Corners::group_vertical(
        index,
        count,
        theme.metrics.corner_lg,
        theme.metrics.corner_xs,
        false,
    );
    let symbol = match entry.kind() {
        RecentKind::Image => Icon::Cube,
        RecentKind::Project => Icon::Folder,
        RecentKind::Remote => Icon::Globe,
    };
    let title = entry.name().to_string();
    let detail = entry.path().map_or_else(
        || entry.target().to_string(),
        |path| path.display().to_string(),
    );
    let tooltip = detail.clone();
    let title_style = TextStyle::new(theme.font(Level::Label), palette.on_surface).truncating();
    let detail_style = TextStyle::new(theme.font(Level::Monospace), palette.outline).truncating();
    grana_ui::canvas(Length::Fill, RECENT_ROW)
        .paint(move |painter, info| {
            let rect = info.rect;
            let fill = palette
                .surface_container
                .lerp(palette.secondary_container, info.hover);
            let row_corners = if info.hover > 0.25 {
                corners.lerp(Corners::all(theme.metrics.corner_md), info.hover)
            } else {
                corners
            };
            painter.rect(rect, row_corners, fill);
            grana_ui::icon::paint(
                symbol,
                painter,
                Vec2::new(rect.min.x + 16.0, rect.center().y),
                14.0,
                palette
                    .primary
                    .lerp(palette.on_secondary_container, info.hover),
                1.6,
            );
            painter.text_in(
                Rect::from_min_max(
                    Vec2::new(rect.min.x + 34.0, rect.min.y + 4.0),
                    Vec2::new(rect.max.x - 26.0, rect.min.y + 22.0),
                ),
                &title,
                title_style,
                Align::Center,
            );
            painter.text_in(
                Rect::from_min_max(
                    Vec2::new(rect.min.x + 34.0, rect.min.y + 21.0),
                    Vec2::new(rect.max.x - 26.0, rect.max.y - 4.0),
                ),
                &detail,
                detail_style,
                Align::Center,
            );
            grana_ui::icon::paint(
                Icon::ChevronRight,
                painter,
                Vec2::new(rect.max.x - 14.0, rect.center().y),
                10.0,
                palette
                    .outline
                    .lerp(palette.on_secondary_container, info.hover),
                1.6,
            );
        })
        .key(Key::index("shell.recent", index))
        .tooltip(&tooltip)
        .press(Message::Command(Command::OpenRecent(index)))
        .cursor(grana_ui::Cursor::Pointing)
        .focus_ring(corners)
}
