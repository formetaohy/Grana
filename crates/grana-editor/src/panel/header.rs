use glam::Vec2;
use grana_session::Command;
use grana_ui::{
    Align, BuildContext, Color, Corners, Edges, Element, Icon, Key, Length, Level, Rect, Role,
    State, TextStyle, icon,
};

use crate::message::Message;
use crate::panel::Layout;
use crate::state::View;
use crate::view::{menu, menu_separator};

pub(super) fn view<'a>(
    ctx: &mut BuildContext<'_, Message>,
    layout: &Layout,
    state: &'a View<'a>,
) -> Vec<Element<'a, Message>> {
    vec![navigation(ctx, layout, state), actions(ctx, layout, state)]
}

fn navigation<'a>(
    ctx: &mut BuildContext<'_, Message>,
    layout: &Layout,
    state: &'a View<'a>,
) -> Element<'a, Message> {
    let hidden = state.workspace.sidebar_hidden;
    let language = ctx.language();
    let tooltip = if hidden {
        grana_i18n::header::show_sidebar(language)
    } else {
        grana_i18n::header::hide_sidebar(language)
    };
    let toggle_role = if hidden { Role::Quiet } else { Role::Tinted };
    let toggle = crate::view::icon_button(
        ctx,
        "sidebar.toggle",
        Icon::Sidebar,
        26.0,
        toggle_role,
        tooltip,
        Message::ToggleSidebar,
    );
    let project = project_pill(ctx, state);
    let open = open_pill(ctx);
    let project_menu = project_menu(ctx);
    let open_menu = open_menu(ctx, state);
    let surface = ctx.theme().surface(Role::Hud, State::Rest);
    grana_ui::row()
        .at(layout.toolbar_left)
        .named("header.navigation")
        .padding(Edges::symmetric(5.0, 4.0))
        .gap(4.0)
        .align(Align::Center)
        .height(Length::Fill)
        .background(surface)
        .children([
            toggle,
            vertical_separator(ctx),
            project,
            vertical_separator(ctx),
            open,
            project_menu,
            open_menu,
        ])
}

fn vertical_separator<'a>(ctx: &mut BuildContext<'_, Message>) -> Element<'a, Message> {
    let color = ctx.theme().palette.separator;
    grana_ui::canvas(Length::Fixed(1.0), 16.0, move |painter, info| {
        painter.rect(info.rect, Corners::ZERO, color);
    })
}

fn settings_menu<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
) -> Element<'a, Message> {
    let key = Key::new("settings.menu");
    let width = 264.0;
    if !ctx.popup_open(key) {
        return grana_ui::popover::popup(key, width, std::iter::empty());
    }
    let language = ctx.language();
    let english = grana_i18n::Language::English;
    let chinese = grana_i18n::Language::Chinese;
    let contrast = grana_ui::widget::toggle_row(
        ctx.theme(),
        grana_i18n::appearance::increase_contrast(language),
        state.workspace.contrast,
    )
    .named("settings.contrast")
    .on_toggle(Message::SetContrast);
    let transparency = grana_ui::widget::toggle_row(
        ctx.theme(),
        grana_i18n::appearance::reduce_transparency(language),
        state.workspace.reduce_transparency,
    )
    .named("settings.transparency")
    .on_toggle(Message::SetReduceTransparency);
    grana_ui::popover::popup(
        key,
        width,
        [
            crate::view::menu_row(
                ctx,
                "settings.language.english",
                english.label(),
                language == english,
                Some(Message::SetLanguage(english)),
            ),
            crate::view::menu_row(
                ctx,
                "settings.language.chinese",
                chinese.label(),
                language == chinese,
                Some(Message::SetLanguage(chinese)),
            ),
            menu_separator(ctx),
            contrast,
            transparency,
        ],
    )
    .persistent()
    .padding(Edges::all(8.0))
    .gap(4.0)
}

fn project_pill<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let palette = theme.palette;
    let radius = theme.metrics.radius;
    let project_font = theme.font(Level::Callout).medium();
    let project = state
        .io
        .project
        .as_deref()
        .unwrap_or(grana_i18n::common::untitled(ctx.language()))
        .to_string();
    let dirty = state.io.dirty;
    let full = ctx
        .measure(&project, TextStyle::new(project_font, Color::WHITE))
        .x;
    let measured = full.min(136.0);
    let width = 10.0 + measured + if dirty { 10.0 } else { 0.0 } + 18.0;
    let key = Key::new("project.menu");
    let open = ctx.popup_open(key);
    let title = project.clone();
    let tooltip = (full > measured).then_some(project.as_str());
    let element = grana_ui::canvas(Length::Fixed(width), 26.0, move |painter, info| {
        let frame = info.rect;
        let engaged = info.hover.max(if open { 1.0 } else { 0.0 });
        if engaged > 0.01 {
            painter.rect(
                frame,
                Corners::all(radius),
                palette.material_hover.amplify(0.65 * engaged),
            );
        }
        let title_left = frame.min.x + 8.0;
        painter.text_in(
            Rect::from_min_max(
                Vec2::new(title_left, frame.min.y),
                Vec2::new(title_left + measured, frame.max.y),
            ),
            &title,
            TextStyle::new(
                project_font,
                palette.text_secondary.lerp(palette.text_primary, engaged),
            )
            .truncating(),
            Align::Center,
        );
        if dirty {
            painter.rect(
                Rect::from_center_size(
                    Vec2::new(title_left + measured + 6.0, frame.center().y),
                    Vec2::splat(4.5),
                ),
                Corners::capsule(),
                palette.warning,
            );
        }
        icon::paint(
            Icon::ChevronDown,
            painter,
            Vec2::new(frame.max.x - 9.0, frame.center().y),
            8.5,
            palette.text_tertiary,
            1.5,
        );
    })
    .key(key)
    .press(Message::Noop)
    .focus_ring(Corners::all(radius));
    match tooltip {
        Some(tooltip) => element.tooltip(tooltip),
        None => element,
    }
}

fn open_pill<'a>(ctx: &mut BuildContext<'_, Message>) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let palette = theme.palette;
    let radius = theme.metrics.radius;
    let font = theme.font(Level::Callout).medium();
    let key = Key::new("open.menu");
    let open = ctx.popup_open(key);
    let label = grana_i18n::header::open(ctx.language());
    grana_ui::canvas(Length::Fixed(78.0), 26.0, move |painter, info| {
        let frame = info.rect;
        let engaged = info.hover.max(if open { 1.0 } else { 0.0 });
        if engaged > 0.01 {
            painter.rect(
                frame,
                Corners::all(radius),
                palette.material_hover.amplify(0.65 * engaged),
            );
        }
        let color = palette.text_secondary.lerp(palette.text_primary, engaged);
        icon::paint(
            Icon::Folder,
            painter,
            Vec2::new(frame.min.x + 12.0, frame.center().y),
            12.5,
            color,
            1.5,
        );
        painter.text_in(
            Rect::from_min_max(
                Vec2::new(frame.min.x + 22.0, frame.min.y),
                Vec2::new(frame.max.x - 16.0, frame.max.y),
            ),
            label,
            TextStyle::new(font, color),
            Align::Center,
        );
        icon::paint(
            Icon::ChevronDown,
            painter,
            Vec2::new(frame.max.x - 9.0, frame.center().y),
            8.5,
            palette.text_tertiary,
            1.5,
        );
    })
    .key(key)
    .press(Message::Noop)
    .focus_ring(Corners::all(radius))
}

fn actions<'a>(
    ctx: &mut BuildContext<'_, Message>,
    layout: &Layout,
    state: &'a View<'a>,
) -> Element<'a, Message> {
    let undo = state.history.undo;
    let redo = state.history.redo;
    let undo_tooltip = undo.map_or_else(
        || grana_i18n::header::nothing_to_undo(ctx.language()).to_string(),
        |action| {
            grana_i18n::header::undo_action(
                ctx.language(),
                crate::text::action(ctx.language(), action),
            )
        },
    );
    let redo_tooltip = redo.map_or_else(
        || grana_i18n::header::nothing_to_redo(ctx.language()).to_string(),
        |action| {
            grana_i18n::header::redo_action(
                ctx.language(),
                crate::text::action(ctx.language(), action),
            )
        },
    );
    let undo_element = grana_ui::widget::icon_button(Icon::Undo, 26.0, Role::Quiet, &undo_tooltip)
        .named("undo")
        .enabled(undo.is_some())
        .press(Message::Command(Command::Undo));
    let redo_element = grana_ui::widget::icon_button(Icon::Redo, 26.0, Role::Quiet, &redo_tooltip)
        .named("redo")
        .enabled(redo.is_some())
        .press(Message::Command(Command::Redo));
    let palette = ctx.theme().palette;
    let pending = state.io.pending > 0 || !state.jobs.is_empty();
    let fit = crate::view::icon_button(
        ctx,
        "view.fit",
        Icon::Fit,
        26.0,
        Role::Quiet,
        grana_i18n::header::frame_volume(ctx.language()),
        Message::FitView,
    );
    let settings = crate::view::icon_button(
        ctx,
        "settings.menu",
        Icon::Sliders,
        26.0,
        Role::Quiet,
        grana_i18n::header::settings(ctx.language()),
        Message::Noop,
    );
    let settings_popup = settings_menu(ctx, state);
    let mut children = vec![
        palette_button(ctx),
        vertical_separator(ctx),
        undo_element,
        redo_element,
        vertical_separator(ctx),
        fit,
        settings,
        vertical_separator(ctx),
        analysis(ctx, state),
        settings_popup,
    ];
    if pending {
        let dot_rect = Rect::from_center_size(
            Vec2::new(
                layout.toolbar_right.max.x - 6.0,
                layout.toolbar_right.min.y + 6.0,
            ),
            Vec2::splat(4.5),
        );
        children.push(
            grana_ui::canvas(Length::Fixed(4.5), 4.5, move |painter, info| {
                painter.rect(info.rect, Corners::capsule(), palette.accent);
            })
            .named("header.pending")
            .at(dot_rect),
        );
    }
    let surface = ctx.theme().surface(Role::Hud, State::Rest);
    grana_ui::row()
        .at(layout.toolbar_right)
        .named("header.actions")
        .padding(Edges::symmetric(5.0, 4.0))
        .gap(3.0)
        .align(Align::Center)
        .justify(Align::End)
        .width(Length::Fill)
        .height(Length::Fill)
        .background(surface)
        .children(children)
}

fn palette_button<'a>(ctx: &mut BuildContext<'_, Message>) -> Element<'a, Message> {
    let shortcut = crate::command::spec("palette.open")
        .and_then(|spec| spec.shortcut)
        .map_or_else(String::new, |shortcut| shortcut.label());
    let tooltip = grana_i18n::header::search_commands(ctx.language(), &shortcut);
    crate::view::icon_button(
        ctx,
        "header.palette",
        Icon::Search,
        26.0,
        Role::Quiet,
        &tooltip,
        Message::OpenPalette,
    )
}

fn analysis<'a>(ctx: &mut BuildContext<'_, Message>, state: &'a View<'a>) -> Element<'a, Message> {
    let open = state.workspace.panel.is_some();
    let role = if open { Role::Prominent } else { Role::Control };
    let message = if open {
        Message::SetPanel(None)
    } else {
        Message::SetPanel(Some(state.workspace.last_panel))
    };
    let label = grana_i18n::header::analysis(ctx.language());
    grana_ui::widget::action_button(ctx.theme(), Icon::Table, label, role)
        .named("header.analysis")
        .press(message)
}

fn open_menu<'a>(ctx: &mut BuildContext<'_, Message>, state: &'a View<'a>) -> Element<'a, Message> {
    let key = Key::new("open.menu");
    if !ctx.popup_open(key) {
        return grana_ui::popover::popup(key, 232.0, std::iter::empty());
    }
    let mut rows = vec![
        command_row(ctx, "open.image"),
        command_row(ctx, "open.folder"),
        command_row(ctx, "open.remote"),
        menu_separator(ctx),
        command_row(ctx, "open.project"),
    ];
    if !state.recent.is_empty() {
        rows.push(menu_separator(ctx));
        rows.push(crate::view::section_header(
            ctx,
            grana_i18n::header::recent(ctx.language()),
            None,
        ));
        for (index, entry) in state.recent.iter().take(crate::RECENT_LIMIT).enumerate() {
            rows.push(crate::view::action_menu_message(
                ctx,
                &format!("open.recent.{index}"),
                match entry.kind() {
                    grana_session::RecentKind::Image => Icon::Cube,
                    grana_session::RecentKind::Project => Icon::Folder,
                    grana_session::RecentKind::Remote => Icon::Globe,
                },
                entry.name(),
                None,
                Message::Command(Command::OpenRecent(index)),
            ));
        }
    }
    menu(ctx, key, 232.0, rows)
}

fn project_menu<'a>(ctx: &mut BuildContext<'_, Message>) -> Element<'a, Message> {
    let key = Key::new("project.menu");
    if !ctx.popup_open(key) {
        return grana_ui::popover::popup(key, 220.0, std::iter::empty());
    }
    let rows = vec![
        command_row(ctx, "save.project"),
        command_row(ctx, "save.as"),
        menu_separator(ctx),
        command_row(ctx, "snapshot"),
        command_row(ctx, "figure"),
        command_row(ctx, "labels"),
    ];
    menu(ctx, key, 220.0, rows)
}

fn command_row<'a>(ctx: &mut BuildContext<'_, Message>, id: &str) -> Element<'a, Message> {
    let spec = crate::command::spec(id).expect("a menu names a registered command");
    let shortcut = spec.shortcut.map(|shortcut| shortcut.label());
    crate::view::action_menu_message(
        ctx,
        id,
        spec.symbol,
        crate::text::command(ctx.language(), id),
        shortcut.as_deref(),
        spec.action.message(),
    )
}
