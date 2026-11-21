use glam::Vec2;
use grana_ui::{
    Align, BuildContext, Corners, Element, Icon, IconStyle, Key, Layer, Length, Level, Rect, Role,
    TextStyle, icon,
};

use crate::message::{Message, UpdateAction};
use crate::state::{Update, UpdateFailure, UpdatePhase, Workspace};

const WIDTH: f32 = 420.0;
const MINIMUM_WIDTH: f32 = 312.0;
const GUTTER: f32 = 24.0;
const PADDING: f32 = 20.0;
const HEADER: f32 = 32.0;
const VERSION: f32 = 28.0;
const STATUS: f32 = 20.0;
const PROGRESS: f32 = 28.0;
const TOGGLE: f32 = 44.0;
const ACTION: f32 = 32.0;
const GAP: f32 = 12.0;
const SYMBOL: f32 = 16.0;

pub fn view<'a>(
    ctx: &mut BuildContext<'_, Message>,
    full: Rect,
    workspace: &Workspace,
) -> Vec<Element<'a, Message>> {
    let open = workspace.update.open;
    let update = &workspace.update;
    let status = status(update, ctx.language());
    let width = (full.width() - 2.0 * GUTTER).clamp(MINIMUM_WIDTH, WIDTH);
    let mut height = PADDING * 2.0 + HEADER + GAP + VERSION + GAP + TOGGLE + GAP + ACTION;
    if status.is_some() {
        height += GAP + STATUS;
    }
    if update.progress.is_some() {
        height += GAP + PROGRESS;
    }
    let size = Vec2::new(width, height);
    let Some(sheet) = crate::dialog::sheet(ctx, full, Key::new("update.dialog"), open, size) else {
        return Vec::new();
    };
    let inner = sheet.rect.shrink(PADDING);
    let inner_width = inner.width();
    let header_rect = Rect::from_min_size(inner.min, Vec2::new(inner_width, HEADER));
    let mut top = header_rect.max.y + GAP;
    let version_rect =
        Rect::from_min_size(Vec2::new(inner.min.x, top), Vec2::new(inner_width, VERSION));
    top = version_rect.max.y + GAP;
    let status_rect = status.as_ref().map(|status| {
        let rect = Rect::from_min_size(Vec2::new(inner.min.x, top), Vec2::new(inner_width, STATUS));
        top = rect.max.y + GAP;
        (status.clone(), rect)
    });
    let progress_rect = update.progress.map(|(done, total)| {
        let rect = Rect::from_min_size(
            Vec2::new(inner.min.x, top),
            Vec2::new(inner_width, PROGRESS),
        );
        top = rect.max.y + GAP;
        (done, total, rect)
    });
    let toggle_rect =
        Rect::from_min_size(Vec2::new(inner.min.x, top), Vec2::new(inner_width, TOGGLE));
    let action_rect = Rect::from_min_size(
        Vec2::new(inner.min.x, inner.max.y - ACTION),
        Vec2::new(inner_width, ACTION),
    );

    let mut children = vec![header(ctx, header_rect, open)];
    children.push(version_row(ctx, update, version_rect));
    if let Some((status, rect)) = status_rect {
        children.push(status_line(ctx, rect, &status));
    }
    if let Some((done, total, rect)) = progress_rect {
        children.push(progress_row(ctx, rect, done, total));
    }
    children.push(auto_row(ctx, update, toggle_rect, open));
    children.push(actions(ctx, update, action_rect, open));

    let backdrop = crate::dialog::scrim(
        ctx,
        full,
        "update.scrim",
        open,
        Layer::Popup,
        Some(Message::Update(UpdateAction::Close)),
    );
    let dialog = grana_ui::column()
        .at(sheet.rect)
        .named("update.dialog")
        .background(sheet.surface)
        .clipped()
        .layer(Layer::Popup)
        .blocks_pointer()
        .modal()
        .children(children);
    vec![backdrop, dialog]
}

fn header<'a>(ctx: &mut BuildContext<'_, Message>, rect: Rect, open: bool) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let palette = theme.palette;
    let language = ctx.language();
    let label = grana_i18n::settings::update(language).to_string();
    let style = TextStyle::new(theme.font(Level::Headline), palette.on_surface);
    let title = grana_ui::canvas(Length::Fill, HEADER)
        .view(label, move |label: &String, painter, info| {
            painter.text_in(
                Rect::from_min_max(
                    info.rect.min,
                    Vec2::new(info.rect.max.x - 38.0, info.rect.max.y),
                ),
                label,
                style,
                Align::Center,
            );
        })
        .named("update.title")
        .at(Rect::from_min_size(
            rect.min,
            Vec2::new(rect.width() - 32.0, rect.height()),
        ));
    let close_rect = Rect::from_center_size(
        Vec2::new(rect.max.x - 16.0, rect.center().y),
        Vec2::splat(32.0),
    );
    let close = grana_ui::canvas(Length::Fixed(32.0), 32.0)
        .view_static(move |painter, info| {
            let corners = Corners::capsule()
                .resolved(info.rect.size())
                .lerp(Corners::all(theme.metrics.corner_sm), info.press);
            painter.rect(
                info.rect,
                corners,
                palette
                    .surface_container_high
                    .lerp(palette.surface_container_highest, info.hover)
                    .lerp(palette.on_surface, 0.12 * info.press),
            );
            icon::paint(
                Icon::Close,
                painter,
                info.rect.center(),
                16.0,
                palette
                    .on_surface_variant
                    .lerp(palette.on_surface, info.hover),
                IconStyle::REGULAR,
            );
        })
        .named("update.close")
        .tooltip(grana_i18n::settings::close(language))
        .cursor(grana_ui::Cursor::Pointing)
        .focus_ring(Corners::capsule())
        .at(close_rect);
    let close = if open {
        close.press(Message::Update(UpdateAction::Close))
    } else {
        close
    };
    grana_ui::column()
        .at(rect)
        .named("update.header")
        .children([title, close])
}

fn version_row<'a>(
    ctx: &mut BuildContext<'_, Message>,
    update: &Update,
    rect: Rect,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let palette = theme.palette;
    let mono = theme.font(Level::Monospace);
    let current = env!("CARGO_PKG_VERSION").to_string();
    let next = update.version.clone();
    grana_ui::canvas(Length::Fill, VERSION)
        .view(
            (current, next),
            move |(current, next): &(String, Option<String>), painter, info| {
                let height = 22.0;
                let held =
                    TextStyle::new(mono, palette.on_secondary_container).aligned(Align::Center);
                let held_width = painter.measure(current, held).x + 18.0;
                let held_rect = Rect::from_min_size(
                    Vec2::new(info.rect.min.x, info.rect.center().y - height * 0.5),
                    Vec2::new(held_width, height),
                );
                painter.rect(held_rect, Corners::capsule(), palette.secondary_container);
                painter.text_in(held_rect, current, held, Align::Center);
                let Some(next) = next else {
                    return;
                };
                let arrow = Vec2::new(held_rect.max.x + 10.0 + SYMBOL * 0.5, info.rect.center().y);
                icon::paint(
                    Icon::ChevronRight,
                    painter,
                    arrow,
                    SYMBOL,
                    palette.on_surface_variant,
                    IconStyle::REGULAR,
                );
                let style =
                    TextStyle::new(mono, palette.on_primary_container).aligned(Align::Center);
                let next_width = painter.measure(next, style).x + 18.0;
                let next_rect = Rect::from_min_size(
                    Vec2::new(
                        arrow.x + SYMBOL * 0.5 + 10.0,
                        info.rect.center().y - height * 0.5,
                    ),
                    Vec2::new(next_width, height),
                );
                painter.rect(next_rect, Corners::capsule(), palette.primary_container);
                painter.text_in(next_rect, next, style, Align::Center);
            },
        )
        .named("update.version")
        .at(rect)
}

fn status_line<'a>(
    ctx: &mut BuildContext<'_, Message>,
    rect: Rect,
    status: &str,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let style =
        TextStyle::new(theme.font(Level::Body), theme.palette.on_surface_variant).truncating();
    let text = status.to_string();
    grana_ui::canvas(Length::Fill, STATUS)
        .view(text, move |text: &String, painter, info| {
            painter.text_in(info.rect, text, style, Align::Center);
        })
        .named("update.status")
        .at(rect)
}

fn progress_row<'a>(
    ctx: &mut BuildContext<'_, Message>,
    rect: Rect,
    done: u64,
    total: u64,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let palette = theme.palette;
    let mono = theme.font(Level::Monospace);
    let fraction = if total == 0 {
        0.0
    } else {
        (done as f32 / total as f32).clamp(0.0, 1.0)
    };
    let label = if total == 0 {
        grana_i18n::settings::downloading(ctx.language()).to_string()
    } else {
        format!(
            "{:.0}% · {:.1} MB",
            fraction * 100.0,
            total as f64 / (1024.0 * 1024.0)
        )
    };
    grana_ui::canvas(Length::Fill, PROGRESS)
        .view(
            (fraction.to_bits(), label),
            move |(fraction, label): &(u32, String), painter, info| {
                let fraction = f32::from_bits(*fraction);
                let track = Rect::from_min_size(
                    Vec2::new(info.rect.min.x, info.rect.max.y - 10.0),
                    Vec2::new(info.rect.width(), 10.0),
                );
                painter.rect(track, Corners::capsule(), palette.surface_container_highest);
                let filled = (track.width() * fraction).max(0.0);
                if filled > 1.0 {
                    painter.rect(
                        Rect::from_min_size(track.min, Vec2::new(filled, track.height())),
                        Corners::capsule(),
                        palette.primary,
                    );
                }
                painter.text_in(
                    Rect::from_min_max(
                        info.rect.min,
                        Vec2::new(info.rect.max.x, info.rect.max.y - 14.0),
                    ),
                    label,
                    TextStyle::new(mono, palette.on_surface_variant),
                    Align::Center,
                );
            },
        )
        .named("update.progress")
        .at(rect)
}

fn auto_row<'a>(
    ctx: &mut BuildContext<'_, Message>,
    update: &Update,
    rect: Rect,
    open: bool,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let label = grana_i18n::settings::automatic(ctx.language());
    let row = grana_ui::widget::toggle_row(&theme, label, update.automatic)
        .named("update.automatic")
        .at(rect)
        .enabled(open);
    if open {
        row.press(Message::Update(UpdateAction::ToggleCheck))
    } else {
        row
    }
}

fn actions<'a>(
    ctx: &mut BuildContext<'_, Message>,
    update: &Update,
    rect: Rect,
    open: bool,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let language = ctx.language();
    let mut buttons = Vec::new();
    let cancel =
        grana_ui::widget::button(&theme, grana_i18n::common::cancel(language), Role::Standard)
            .named("update.cancel")
            .height(Length::Fixed(ACTION));
    let check =
        grana_ui::widget::button(&theme, grana_i18n::settings::check(language), Role::Filled)
            .named("update.check")
            .height(Length::Fixed(ACTION));
    let download = grana_ui::widget::button(
        &theme,
        grana_i18n::settings::download(language),
        Role::Filled,
    )
    .named("update.download")
    .height(Length::Fixed(ACTION));
    let restart = grana_ui::widget::button(
        &theme,
        grana_i18n::settings::restart(language),
        Role::Filled,
    )
    .named("update.restart")
    .height(Length::Fixed(ACTION));
    let notes = grana_ui::widget::button(
        &theme,
        grana_i18n::settings::notes(language),
        Role::Standard,
    )
    .named("update.notes")
    .height(Length::Fixed(ACTION));
    match update.phase {
        UpdatePhase::Checking => buttons.push((cancel, UpdateAction::Close)),
        UpdatePhase::Downloading => buttons.push((
            grana_ui::widget::button(&theme, grana_i18n::common::cancel(language), Role::Standard)
                .named("update.cancel")
                .height(Length::Fixed(ACTION)),
            UpdateAction::Cancel,
        )),
        UpdatePhase::Available => {
            if update.notes.is_some() {
                buttons.push((notes, UpdateAction::Notes));
            }
            buttons.push((download, UpdateAction::Download));
        }
        UpdatePhase::Ready => {
            buttons.push((cancel, UpdateAction::Close));
            buttons.push((restart, UpdateAction::Apply));
        }
        UpdatePhase::Idle | UpdatePhase::Current | UpdatePhase::Failed(_) => {
            buttons.push((cancel, UpdateAction::Close));
            buttons.push((check, UpdateAction::Check));
        }
    }
    let buttons = buttons
        .into_iter()
        .map(|(element, action)| element.press(Message::Update(action)))
        .collect::<Vec<_>>();
    let buttons = if open {
        buttons
    } else {
        buttons
            .into_iter()
            .map(|element| element.enabled(false))
            .collect()
    };
    grana_ui::row()
        .at(rect)
        .gap(8.0)
        .align(Align::Center)
        .justify(Align::End)
        .children(buttons)
}

fn status(update: &Update, language: grana_i18n::Language) -> Option<String> {
    match update.phase {
        UpdatePhase::Idle => update.size.map(size_text),
        UpdatePhase::Available => update.size.map(size_text),
        UpdatePhase::Checking => Some(grana_i18n::settings::checking(language).to_string()),
        UpdatePhase::Current => Some(grana_i18n::settings::up_to_date(language).to_string()),
        UpdatePhase::Downloading => None,
        UpdatePhase::Ready => Some(grana_i18n::settings::ready(language).to_string()),
        UpdatePhase::Failed(failure) => Some(failure_text(failure, language).to_string()),
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
