use glam::Vec2;
use grana_ui::{
    Align, BuildContext, Corners, Edges, Element, Fill, Icon, IconStyle, Length, Level, Rect, Role,
    State, TextStyle,
};

use super::{bytes, row};
use crate::message::{Message, UpdateAction};
use crate::state::{Update, UpdateFailure, UpdatePhase, Workspace};

const HERO_INSET: f32 = 18.0;
const HERO_GAP: f32 = 14.0;
const BADGE: f32 = 44.0;
const BADGE_ICON: f32 = 22.0;
const ACTION_WIDTH: f32 = 158.0;
const ACTION_GAP: f32 = 16.0;
const PROGRESS: f32 = 4.0;

pub(super) fn view<'a>(
    ctx: &mut BuildContext<'_, Message>,
    workspace: &Workspace,
    open: bool,
) -> Vec<Element<'a, Message>> {
    let language = ctx.language();
    let update = &workspace.update;
    let mut rows = vec![
        row::switch(
            ctx,
            Icon::Clock,
            grana_i18n::settings::automatic(language),
            update.automatic,
            open,
            |_| Message::Update(UpdateAction::ToggleCheck),
        )
        .named("settings.automatic"),
    ];
    if update.notes.is_some() {
        rows.push(
            row::action(
                ctx,
                Icon::Globe,
                grana_i18n::settings::notes(language),
                None,
                Icon::ChevronRight,
                Message::Update(UpdateAction::Notes),
                open,
            )
            .named("settings.notes"),
        );
    }
    vec![
        hero(ctx, update, open).named("settings.update"),
        row::section(ctx, None, rows),
    ]
}

fn hero<'a>(
    ctx: &mut BuildContext<'_, Message>,
    update: &Update,
    open: bool,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let palette = theme.palette;
    let language = ctx.language();
    let failed = matches!(update.phase, UpdatePhase::Failed(_));
    let badge_fill = if failed {
        palette.error_container
    } else {
        palette.primary_container
    };
    let badge_color = if failed {
        palette.on_error_container
    } else {
        palette.on_primary_container
    };
    let badge_symbol = if failed {
        Icon::Warning
    } else {
        Icon::Sparkles
    };
    let badge = grana_ui::canvas(Length::Fixed(BADGE), BADGE).view_static(move |painter, info| {
        painter.rect(info.rect, Corners::capsule(), badge_fill);
        grana_ui::icon::paint(
            badge_symbol,
            painter,
            info.rect.center(),
            BADGE_ICON,
            badge_color,
            IconStyle::REGULAR,
        );
    });
    let title = ctx
        .text(
            format!(
                "{} {}",
                grana_i18n::settings::version(language),
                env!("CARGO_PKG_VERSION")
            ),
            TextStyle::new(theme.font(Level::Title), palette.on_surface),
        )
        .width(Length::Fill);
    let mut lines = vec![title];
    if let Some(detail) = status(update, language) {
        let color = if failed {
            palette.error
        } else {
            palette.on_surface_variant
        };
        lines.push(
            ctx.text(
                detail,
                TextStyle::new(theme.font(Level::Caption), color).truncating(),
            )
            .width(Length::Fill),
        );
    }
    let (label, action, role) = action(update, language);
    let control = ctx
        .button_width(label, role, Length::Fixed(ACTION_WIDTH))
        .named("settings.update.action")
        .enabled(open)
        .press(Message::Update(action));
    let head = grana_ui::row()
        .width(Length::Fill)
        .gap(ACTION_GAP)
        .align(Align::Center)
        .children([
            badge,
            grana_ui::column()
                .width(Length::Fill)
                .gap(2.0)
                .children(lines),
            control,
        ]);
    let mut body = vec![head];
    if let Some((done, total)) = update.progress
        && matches!(update.phase, UpdatePhase::Downloading)
    {
        body.push(progress(ctx, done, total));
    }
    let mut surface = theme.surface(Role::CardElevated, State::Rest);
    surface.fill = Fill::Solid(palette.surface_container_high);
    surface.corners = Corners::all(24.0);
    grana_ui::column()
        .width(Length::Fill)
        .padding(Edges::all(HERO_INSET))
        .gap(HERO_GAP)
        .background(surface)
        .children(body)
}

fn progress<'a>(
    ctx: &mut BuildContext<'_, Message>,
    done: u64,
    total: u64,
) -> Element<'a, Message> {
    let palette = ctx.theme().palette;
    let target = if total == 0 {
        0.0
    } else {
        (done as f64 / total as f64).clamp(0.0, 1.0) as f32
    };
    let fraction = ctx.animate(
        grana_ui::Key::new("settings.update.progress").id(),
        target,
        grana_ui::Motion::effect_fast(),
    );
    let track = palette.on_surface.with_alpha(0.12);
    let fill = palette.primary;
    grana_ui::canvas(Length::Fill, PROGRESS).view_static(move |painter, info| {
        painter.rect(info.rect, Corners::capsule(), track);
        if fraction > 0.0 {
            let width = (info.rect.width() * fraction).max(info.rect.height());
            painter.rect(
                Rect::from_min_size(info.rect.min, Vec2::new(width, info.rect.height())),
                Corners::capsule(),
                fill,
            );
        }
    })
}

fn action(update: &Update, language: grana_i18n::Language) -> (&'static str, UpdateAction, Role) {
    match update.phase {
        UpdatePhase::Idle | UpdatePhase::Current | UpdatePhase::Failed(_) => (
            grana_i18n::settings::check(language),
            UpdateAction::Check,
            Role::Filled,
        ),
        UpdatePhase::Checking | UpdatePhase::Downloading => (
            grana_i18n::common::cancel(language),
            UpdateAction::Cancel,
            Role::Tonal,
        ),
        UpdatePhase::Available => (
            grana_i18n::settings::download(language),
            UpdateAction::Download,
            Role::Filled,
        ),
        UpdatePhase::Ready => (
            grana_i18n::settings::restart(language),
            UpdateAction::Apply,
            Role::Filled,
        ),
    }
}

fn status(update: &Update, language: grana_i18n::Language) -> Option<String> {
    match update.phase {
        UpdatePhase::Idle => None,
        UpdatePhase::Checking => Some(grana_i18n::settings::checking(language).to_string()),
        UpdatePhase::Current => Some(grana_i18n::settings::up_to_date(language).to_string()),
        UpdatePhase::Available => update.version.clone().map(|version| match update.size {
            Some(size) => format!("{version} · {}", bytes(size)),
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
            "{:.0}% · {}",
            done as f64 / total as f64 * 100.0,
            bytes(total)
        )
    }
}

fn failure_text(failure: UpdateFailure, language: grana_i18n::Language) -> &'static str {
    match failure {
        UpdateFailure::Offline => grana_i18n::settings::offline(language),
        UpdateFailure::Corrupt => grana_i18n::settings::corrupt(language),
        UpdateFailure::Signature => grana_i18n::settings::signature(language),
        UpdateFailure::Storage => grana_i18n::settings::storage_failure(language),
        UpdateFailure::Installer => grana_i18n::settings::installer(language),
        UpdateFailure::Other => grana_i18n::settings::failure(language),
    }
}
