use glam::Vec2;
use grana_session::{Command, Kind, RecentEntry, RecentKind, RecoveryView, SessionView};
use grana_ui::{
    Align, BuildContext, Corners, Edges, Element, Fill, Icon, IconStyle, Key, Layer, Length, Level,
    Rect, Role, State, TextStyle,
};

use crate::message::Message;
use crate::state::Workspace;

const PANEL_WIDTH: f32 = 440.0;
const PANEL_PADDING: f32 = 20.0;
const PANEL_GAP: f32 = 14.0;
const HERO_HEIGHT: f32 = 42.0;
const RECENT_HEADER: f32 = 20.0;
const RECENT_ROW: f32 = 44.0;
const STITCH_GAP: f32 = 8.0;

pub const STARTUP_SIZE: Vec2 = Vec2::new(PANEL_WIDTH, 180.0);

struct Status {
    text: String,
    detail: Option<String>,
    busy: bool,
    error: bool,
}

pub(crate) fn window_size(panel: Vec2) -> Vec2 {
    Vec2::new(PANEL_WIDTH, panel.y.max(168.0))
}

pub(crate) fn view<'a>(
    ctx: &mut BuildContext<'_, Message>,
    view: &SessionView<'_>,
    workspace: &Workspace,
    full: Rect,
) -> Vec<Element<'a, Message>> {
    let theme = *ctx.theme();
    let palette = theme.palette;
    let shown = view.recent.len().min(crate::RECENT_LIMIT);
    let glow = Rect::from_min_size(full.min, Vec2::new(full.width(), 0.62 * full.height()));
    let primary = palette.primary;
    let panel_color = palette.surface_container_low;
    let mut elements = vec![
        grana_ui::canvas(Length::Fill, full.height())
            .view_static(move |painter, info| {
                painter.rect(info.rect, Corners::ZERO, panel_color);
                painter.gradient(
                    glow,
                    Corners::ZERO,
                    panel_color.lerp(primary, 0.08),
                    panel_color,
                );
            })
            .named("shell.backdrop")
            .at(full)
            .layer(Layer::Content),
    ];
    let mut children = vec![hero_header(ctx, workspace)];
    if let Some(status) = status(view, workspace.language) {
        children.push(status_banner(ctx, &status));
    }
    let mut popups = Vec::new();
    match view.stitch.as_ref() {
        Some(stitch) => children.push(stitch_block(ctx, stitch, &mut popups)),
        None => {
            if let Some(recovery) = view.recovery.as_ref() {
                children.push(recovery_block(ctx, recovery));
            }
            children.push(open_bento(ctx, &mut popups));
        }
    }
    if shown > 0 {
        children.push(recent_block(ctx, view));
    }
    let top_pad = if workspace.chrome.leading > 0.0 {
        PANEL_PADDING + 14.0
    } else {
        PANEL_PADDING
    };
    let panel = grana_ui::column()
        .padding(Edges {
            left: PANEL_PADDING,
            top: top_pad,
            right: PANEL_PADDING,
            bottom: PANEL_PADDING,
        })
        .gap(PANEL_GAP)
        .width(Length::Fixed(PANEL_WIDTH))
        .children(children)
        .named("shell.panel");
    elements.push(
        grana_ui::column()
            .padding(Edges {
                left: ((full.width() - PANEL_WIDTH) * 0.5).max(0.0),
                top: 0.0,
                right: 0.0,
                bottom: 0.0,
            })
            .at(full)
            .children([panel]),
    );
    elements.extend(popups);
    elements.extend(crate::panel::remote(ctx, full, workspace.remote_open));
    elements
}

fn hero_header<'a>(
    ctx: &mut BuildContext<'_, Message>,
    workspace: &Workspace,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let palette = theme.palette;
    let brand = grana_ui::canvas(Length::Fixed(40.0), 40.0)
        .view_static(|painter, info| {
            let badge = Rect::from_center_size(info.rect.center(), Vec2::splat(36.0));
            grana_brand::paint(painter, badge);
        })
        .named("shell.brand");
    let close = workspace.chrome.controls && workspace.chrome.close_only;
    let trail_width = if close { 32.0 } else { 0.0 };
    let display = theme.font(Level::Display);
    let title_width = PANEL_WIDTH - 2.0 * PANEL_PADDING - 40.0 - 12.0 - trail_width;
    let title = grana_ui::canvas(Length::Fixed(title_width), HERO_HEIGHT).view_static(
        move |painter, info| {
            painter.text_in(
                info.rect,
                "Grana",
                TextStyle::new(display, palette.on_surface),
                Align::Center,
            );
        },
    );
    let mut row = vec![brand, title];
    if close {
        row.push(crate::chrome::close_button(ctx, 32.0));
    }
    grana_ui::row()
        .gap(12.0)
        .align(Align::Center)
        .height(Length::Fixed(HERO_HEIGHT))
        .children(row)
}

fn status_banner<'a>(ctx: &mut BuildContext<'_, Message>, status: &Status) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let palette = theme.palette;
    let (bg, fg) = if status.error {
        (palette.error_container, palette.on_error_container)
    } else {
        (palette.surface_container, palette.on_surface)
    };
    let mut row = Vec::new();
    if status.busy {
        row.push(crate::view::spinner(ctx, 16.0));
    } else if status.error {
        row.push(grana_ui::widget::icon(
            Icon::Warning,
            16.0,
            fg,
            IconStyle::REGULAR,
        ));
    }
    row.push(
        grana_ui::text(
            status.text.clone(),
            TextStyle::new(theme.font(Level::Label), fg).truncating(),
        )
        .width(Length::Fill),
    );
    if let Some(detail) = &status.detail {
        row.push(grana_ui::text(
            detail.clone(),
            TextStyle::new(theme.font(Level::Monospace), fg.with_alpha(0.8)),
        ));
    }
    let mut surface = theme.surface(Role::Card, State::Rest);
    surface.fill = Fill::Solid(bg);
    surface.corners = Corners::all(theme.metrics.corner_md);
    grana_ui::row()
        .width(Length::Fill)
        .padding(Edges::symmetric(14.0, 10.0))
        .gap(8.0)
        .align(Align::Center)
        .background(surface)
        .children(row)
}

fn open_bento<'a>(
    ctx: &mut BuildContext<'_, Message>,
    popups: &mut Vec<Element<'a, Message>>,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let language = ctx.language();
    let outer = theme.metrics.corner_lg;
    let inner = theme.metrics.corner_xs;
    let gap = theme.metrics.group_gap;
    let content_width = PANEL_WIDTH - 2.0 * PANEL_PADDING;
    let menu_key = Key::new("shell.open.menu");
    let open = ctx.popup_open(menu_key);
    let height = 40.0;
    let lead_w = content_width - height - gap;
    let lead = grana_ui::widget::action_button(
        &theme,
        Icon::Cube,
        grana_i18n::shell::open_anything(language),
        Role::Filled,
    )
    .shaped(Corners::new(outer, inner, inner, inner))
    .width(Length::Fixed(lead_w))
    .height(Length::Fixed(height))
    .named("shell.open.any")
    .press(Message::Command(Command::OpenAny));
    let trail = grana_ui::widget::icon_button(
        if open {
            Icon::ChevronUp
        } else {
            Icon::ChevronDown
        },
        height,
        Role::Filled,
        "",
    )
    .shaped(if open {
        Corners::capsule()
    } else {
        Corners::new(inner, outer, inner, inner)
    })
    .key(menu_key)
    .press(Message::Noop);
    if open {
        let rows = vec![
            crate::view::action_menu_message(
                ctx,
                "shell.menu.folder",
                Icon::Folder,
                grana_i18n::shell::folder(language),
                None,
                Message::Command(Command::OpenDataset),
            ),
            crate::view::action_menu_message(
                ctx,
                "shell.menu.remote",
                Icon::Globe,
                grana_i18n::shell::remote(language),
                None,
                Message::BeginRemote,
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
        .gap(gap)
        .align(Align::Center)
        .height(Length::Fixed(height))
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
    let detail = if stitch.scanning {
        grana_i18n::shell::scanning_tiles(language).to_string()
    } else if stitch.running {
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
    let confirm = if stitch.running || stitch.scanning {
        Message::Noop
    } else {
        Message::Command(Command::ConfirmStitch)
    };
    let label = if stitch.scanning {
        grana_i18n::shell::scanning_tiles(language).to_string()
    } else if stitch.running {
        grana_i18n::shell::stitching(language, stitch.tiles)
    } else {
        grana_i18n::shell::stitch(language).to_string()
    };
    let mut row = Vec::new();
    if stitch.running || stitch.scanning {
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

fn status(view: &SessionView<'_>, language: grana_i18n::Language) -> Option<Status> {
    if let Some((label, done, total)) = view.io.document {
        return Some(Status {
            text: grana_i18n::common::task(language, label),
            detail: (total > 0).then(|| grana_i18n::shell::steps_complete(language, done, total)),
            busy: true,
            error: false,
        });
    }
    if let Some(label) = view.io.loading {
        return Some(Status {
            text: grana_i18n::shell::opening(language, label),
            detail: None,
            busy: true,
            error: false,
        });
    }
    if let Some((label, done, total)) = view.io.label {
        return Some(Status {
            text: grana_i18n::common::task(language, label),
            detail: (total > 0).then(|| grana_i18n::shell::steps_stored(language, done, total)),
            busy: true,
            error: false,
        });
    }
    if let Some(failure) = view.io.failure {
        return Some(Status {
            text: failure.to_string(),
            detail: None,
            busy: false,
            error: true,
        });
    }
    if let Some(message) = view.io.message {
        return Some(Status {
            text: message.text().to_string(),
            detail: None,
            busy: false,
            error: message.kind() == Kind::Error,
        });
    }
    None
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
    let inner = theme.metrics.corner_xs;
    let gap = theme.metrics.group_gap;
    grana_ui::column()
        .padding(Edges::all(12.0))
        .gap(8.0)
        .background(surface)
        .children([
            grana_ui::row()
                .width(Length::Fill)
                .align(Align::Center)
                .children([
                    grana_ui::text(
                        headline,
                        TextStyle::new(theme.font(Level::Title), palette.on_tertiary_container)
                            .truncating(),
                    )
                    .width(Length::Fill),
                    grana_ui::text(
                        detail,
                        TextStyle::new(
                            theme.font(Level::Caption),
                            palette.on_tertiary_container.with_alpha(0.78),
                        )
                        .truncating(),
                    ),
                ]),
            grana_ui::row().gap(gap).children([
                grana_ui::widget::action_button(
                    &theme,
                    Icon::Undo,
                    grana_i18n::shell::recover(language),
                    Role::Filled,
                )
                .shaped(Corners::split_leading(inner))
                .width(Length::Fill)
                .named("shell.recover")
                .press(Message::Command(Command::RecoverAnalysis)),
                grana_ui::widget::action_button(
                    &theme,
                    Icon::Trash,
                    grana_i18n::shell::discard(language),
                    Role::SurfaceHigh,
                )
                .shaped(Corners::split_trailing(inner))
                .width(Length::Fill)
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
            .view(recent_label, move |_, painter, info| {
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
        .view(
            (title.clone(), detail.clone(), symbol, index, count),
            move |_, painter, info| {
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
                let badge = Rect::from_center_size(
                    Vec2::new(rect.min.x + 21.0, rect.center().y),
                    Vec2::splat(26.0),
                );
                painter.rect(
                    badge,
                    Corners::all(8.0),
                    palette
                        .surface_container_high
                        .lerp(palette.primary.with_alpha(0.18), info.hover),
                );
                grana_ui::icon::paint(
                    symbol,
                    painter,
                    badge.center(),
                    15.0,
                    palette
                        .primary
                        .lerp(palette.on_secondary_container, info.hover),
                    IconStyle::REGULAR,
                );
                painter.text_in(
                    Rect::from_min_max(
                        Vec2::new(rect.min.x + 42.0, rect.min.y + 4.0),
                        Vec2::new(rect.max.x - 28.0, rect.min.y + 22.0),
                    ),
                    &title,
                    title_style,
                    Align::Center,
                );
                painter.text_in(
                    Rect::from_min_max(
                        Vec2::new(rect.min.x + 42.0, rect.min.y + 22.0),
                        Vec2::new(rect.max.x - 28.0, rect.max.y - 4.0),
                    ),
                    &detail,
                    detail_style,
                    Align::Center,
                );
                grana_ui::icon::paint(
                    Icon::ChevronRight,
                    painter,
                    Vec2::new(rect.max.x - 15.0, rect.center().y),
                    16.0,
                    palette
                        .outline
                        .lerp(palette.on_secondary_container, info.hover),
                    IconStyle::REGULAR,
                );
            },
        )
        .key(Key::index("shell.recent", index))
        .tooltip(&tooltip)
        .press(Message::Command(Command::OpenRecent(index)))
        .cursor(grana_ui::Cursor::Pointing)
        .focus_ring(corners)
}
