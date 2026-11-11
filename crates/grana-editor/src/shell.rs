use glam::Vec2;
use grana_session::{Command, Kind, RecentEntry, RecentKind, RecoveryView, SessionView};
use grana_ui::{
    Align, BuildContext, Corners, Edges, Element, GlassStyle, Icon, Key, Layer, Length, Level,
    Rect, Role, TextStyle, TypeStyle,
};

use crate::message::Message;
use crate::state::Workspace;

const CARD_WIDTH: f32 = 468.0;
const CARD_PADDING: f32 = 24.0;
const CARD_GAP: f32 = 14.0;
const BRAND_HEIGHT: f32 = 42.0;
const STATE_HEIGHT: f32 = 24.0;
const RECENT_HEADER: f32 = 22.0;
const RECENT_ROW: f32 = 38.0;
const RECOVERY_HEIGHT: f32 = 76.0;
const LANGUAGE_HEIGHT: f32 = 28.0;

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
    let palette = ctx.theme().palette;
    let backdrop = palette.scene_backdrop;
    let status = status(view, workspace.language);
    let language = workspace.language;
    let detail_style = TextStyle::new(
        ctx.theme().font(Level::Callout),
        if status.error {
            palette.warning
        } else {
            palette.text_tertiary
        },
    )
    .wrapping();
    let detail = ctx.layout_text(
        &status.detail,
        detail_style,
        CARD_WIDTH - 2.0 * CARD_PADDING,
    );
    let detail_height = detail.size.y.max(18.0);
    let button = ctx.theme().metrics.control_height;
    let shown = view.recent.len().min(crate::RECENT_LIMIT);
    let recents = if shown > 0 {
        CARD_GAP + RECENT_HEADER + RECENT_ROW * shown as f32 + 2.0 * (shown as f32 - 1.0)
    } else {
        0.0
    };
    let recovered = if view.recovery.is_some() {
        CARD_GAP + RECOVERY_HEIGHT
    } else {
        0.0
    };
    let height = 2.0 * CARD_PADDING
        + BRAND_HEIGHT
        + CARD_GAP
        + STATE_HEIGHT
        + CARD_GAP
        + detail_height
        + CARD_GAP
        + button
        + CARD_GAP
        + button
        + recovered
        + CARD_GAP
        + LANGUAGE_HEIGHT
        + recents;
    let rect = Rect::from_min_size(
        Vec2::new(
            full.center().x - 0.5 * CARD_WIDTH,
            full.center().y - 0.54 * height,
        ),
        Vec2::new(CARD_WIDTH, height),
    );
    let glow = Rect::from_min_size(full.min, Vec2::new(full.width(), 0.62 * full.height()));
    let accent = palette.accent;
    let mut elements = vec![
        grana_ui::canvas(Length::Fill, full.height(), move |painter, info| {
            painter.rect(info.rect, Corners::ZERO, backdrop);
            painter.gradient(glow, Corners::ZERO, backdrop.lerp(accent, 0.16), backdrop);
        })
        .named("shell.backdrop")
        .at(full)
        .layer(Layer::Content),
    ];
    let mut children = vec![
        brand(ctx),
        state(ctx, &status),
        grana_ui::text(status.detail.clone(), detail_style),
        actions(ctx),
    ];
    if let Some(recovery) = view.recovery.as_ref() {
        children.push(recovery_block(ctx, recovery));
    }
    children.push(
        grana_ui::row().gap(8.0).children([
            grana_ui::widget::action_button(
                ctx.theme(),
                Icon::Folder,
                grana_i18n::shell::ome_zarr_folder(language),
                Role::Control,
            )
            .named("shell.open.dataset")
            .press(Message::Command(Command::OpenDataset)),
            grana_ui::widget::action_button(
                ctx.theme(),
                Icon::Globe,
                grana_i18n::shell::remote_ome_zarr(language),
                Role::Control,
            )
            .named("shell.open.remote")
            .press(Message::BeginRemote),
            grana_ui::widget::action_button(
                ctx.theme(),
                Icon::Folder,
                grana_i18n::shell::project(language),
                Role::Control,
            )
            .named("shell.open.project")
            .press(Message::Command(Command::OpenProject)),
        ]),
    );
    children.push(language_switch(ctx));
    children.push(recent_block(ctx, view));
    let card = grana_ui::column()
        .padding(Edges::all(CARD_PADDING))
        .gap(CARD_GAP)
        .children(children)
        .at(rect)
        .glass(GlassStyle::surface())
        .named("shell.card");
    elements.push(card);
    elements.extend(crate::panel::remote(ctx, full, workspace.remote_open));
    elements
}

fn status(view: &SessionView<'_>, language: grana_i18n::Language) -> Status {
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

fn brand<'a>(ctx: &mut BuildContext<'_, Message>) -> Element<'a, Message> {
    let palette = ctx.theme().palette;
    let title = ctx.theme().font(Level::Title).semibold();
    let caption = ctx.theme().font(Level::Caption);
    let version = format!("v{}", env!("CARGO_PKG_VERSION"));
    grana_ui::canvas(Length::Fill, BRAND_HEIGHT, move |painter, info| {
        let rect = info.rect;
        let mut y = rect.center().y - 8.0;
        for width in [6.5, 7.6, 8.7, 9.8, 10.9] {
            painter.rect(
                Rect::from_min_size(Vec2::new(rect.min.x, y), Vec2::new(width, 2.4)),
                Corners::capsule(),
                palette.brand,
            );
            y += 3.8;
        }
        painter.text_in(
            Rect::from_min_size(
                Vec2::new(rect.min.x + 20.0, rect.min.y),
                Vec2::new(180.0, rect.height()),
            ),
            "Grana",
            TextStyle::new(title, palette.text_primary),
            Align::Center,
        );
        painter.text_in(
            Rect::from_min_size(
                Vec2::new(rect.max.x - 140.0, rect.min.y),
                Vec2::new(140.0, rect.height()),
            ),
            &version,
            TextStyle::new(caption, palette.text_quaternary).aligned(Align::End),
            Align::Center,
        );
    })
    .named("shell.brand")
}

fn state<'a>(ctx: &mut BuildContext<'_, Message>, status: &Status) -> Element<'a, Message> {
    let palette = ctx.theme().palette;
    let font = ctx.theme().font(Level::Headline).medium();
    let color = if status.error {
        palette.warning
    } else {
        palette.text_secondary
    };
    let mut children = Vec::new();
    if status.busy {
        children.push(crate::view::spinner(ctx, 16.0));
    }
    children.push(grana_ui::text(
        status.text.clone(),
        TextStyle::new(font, color).truncating(),
    ));
    grana_ui::row()
        .gap(10.0)
        .align(Align::Center)
        .height(Length::Fixed(STATE_HEIGHT))
        .children(children)
}

fn actions<'a>(ctx: &mut BuildContext<'_, Message>) -> Element<'a, Message> {
    grana_ui::widget::button_width(
        ctx.theme(),
        grana_i18n::shell::open_image(ctx.language()),
        Role::Prominent,
        Length::Fill,
    )
    .named("shell.open.image")
    .press(Message::Command(Command::OpenImage))
}

fn language_switch<'a>(ctx: &mut BuildContext<'_, Message>) -> Element<'a, Message> {
    let language = ctx.language();
    let labels = grana_i18n::Language::ALL.map(grana_i18n::Language::label);
    let selected = grana_i18n::Language::ALL
        .iter()
        .position(|candidate| *candidate == language)
        .unwrap_or(0);
    let caption = TextStyle::new(
        ctx.theme().font(Level::Callout),
        ctx.theme().palette.text_tertiary,
    );
    grana_ui::row()
        .gap(10.0)
        .align(Align::Center)
        .height(Length::Fixed(LANGUAGE_HEIGHT))
        .children([
            grana_ui::text(grana_i18n::appearance::language(language), caption),
            crate::view::segmented_fill(ctx, "shell.language", &labels, selected, |index| {
                match grana_i18n::Language::ALL.get(index) {
                    Some(language) => Message::SetLanguage(*language),
                    None => Message::Noop,
                }
            }),
        ])
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
    grana_ui::column()
        .gap(6.0)
        .height(Length::Fixed(RECOVERY_HEIGHT))
        .children([
            grana_ui::text(
                headline,
                TextStyle::new(theme.font(Level::Body), palette.text_primary).truncating(),
            ),
            grana_ui::text(
                detail,
                TextStyle::new(theme.font(Level::Caption), palette.text_tertiary).truncating(),
            ),
            grana_ui::row().gap(8.0).children([
                grana_ui::widget::action_button(
                    &theme,
                    Icon::Undo,
                    grana_i18n::shell::recover(language),
                    Role::Prominent,
                )
                .named("shell.recover")
                .press(Message::Command(Command::RecoverAnalysis)),
                grana_ui::widget::action_button(
                    &theme,
                    Icon::Trash,
                    grana_i18n::shell::discard(language),
                    Role::Control,
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
    let palette = ctx.theme().palette;
    let header_style = TextStyle::new(
        TypeStyle {
            tracking: 0.7,
            ..TypeStyle::new(10.0, 600.0, 14.0)
        },
        palette.text_tertiary,
    );
    let recent_label = grana_i18n::shell::recent(ctx.language());
    let mut children = vec![
        grana_ui::canvas(Length::Fill, RECENT_HEADER, move |painter, info| {
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
    for (index, entry) in entries.iter().enumerate() {
        children.push(recent_row(ctx, index, entry));
    }
    grana_ui::column().gap(2.0).children(children)
}

fn recent_row<'a>(
    ctx: &mut BuildContext<'_, Message>,
    index: usize,
    entry: &RecentEntry,
) -> Element<'a, Message> {
    let palette = ctx.theme().palette;
    let theme = *ctx.theme();
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
    let title_style = TextStyle::new(theme.font(Level::Callout), palette.text_primary).truncating();
    let detail_style =
        TextStyle::new(theme.font(Level::Caption), palette.text_tertiary).truncating();
    grana_ui::canvas(Length::Fill, RECENT_ROW, move |painter, info| {
        let rect = info.rect;
        if info.hover > 0.01 {
            painter.rect(
                rect,
                Corners::all(9.0),
                palette.selected.amplify(0.8 * info.hover),
            );
        }
        grana_ui::icon::paint(
            symbol,
            painter,
            Vec2::new(rect.min.x + 14.0, rect.center().y),
            14.0,
            palette
                .text_secondary
                .lerp(palette.text_primary, info.hover),
            1.5,
        );
        painter.text_in(
            Rect::from_min_max(
                Vec2::new(rect.min.x + 30.0, rect.min.y + 2.0),
                Vec2::new(rect.max.x - 22.0, rect.min.y + 21.0),
            ),
            &title,
            title_style,
            Align::Center,
        );
        painter.text_in(
            Rect::from_min_max(
                Vec2::new(rect.min.x + 30.0, rect.min.y + 19.0),
                Vec2::new(rect.max.x - 22.0, rect.max.y - 2.0),
            ),
            &detail,
            detail_style,
            Align::Center,
        );
        grana_ui::icon::paint(
            Icon::ChevronRight,
            painter,
            Vec2::new(rect.max.x - 12.0, rect.center().y),
            9.0,
            palette
                .text_quaternary
                .lerp(palette.text_tertiary, info.hover),
            1.5,
        );
    })
    .key(Key::index("shell.recent", index))
    .tooltip(&tooltip)
    .press(Message::Command(Command::OpenRecent(index)))
    .cursor(grana_ui::Cursor::Pointing)
    .focus_ring(Corners::all(9.0))
}
