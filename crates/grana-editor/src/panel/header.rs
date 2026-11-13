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
    let toggle_role = if hidden {
        Role::SurfaceHigh
    } else {
        Role::Tonal
    };
    let toggle_corners = if hidden {
        Corners::capsule()
    } else {
        Corners::all(10.0)
    };
    let toggle = crate::view::icon_button(
        ctx,
        "sidebar.toggle",
        Icon::Sidebar,
        30.0,
        toggle_role,
        tooltip,
        Message::ToggleSidebar,
    )
    .shaped(toggle_corners);
    let project = project_split(ctx, state);
    let open = open_split(ctx);
    let project_menu = project_menu(ctx, state);
    let open_menu = open_menu(ctx, state);
    let surface = ctx.theme().surface(Role::FloatingToolbar, State::Rest);
    grana_ui::row()
        .at(layout.toolbar_left)
        .named("header.navigation")
        .padding(Edges::symmetric(6.0, 6.0))
        .gap(6.0)
        .align(Align::Center)
        .height(Length::Fill)
        .background(surface)
        .children([toggle, project, open, project_menu, open_menu])
}

fn settings_menu<'a>(ctx: &mut BuildContext<'_, Message>) -> Element<'a, Message> {
    let key = Key::new("settings.menu");
    let width = 264.0;
    if !ctx.popup_open(key) {
        return grana_ui::popover::popup(key, width, std::iter::empty());
    }
    let language = ctx.language();
    let english = grana_i18n::Language::English;
    let chinese = grana_i18n::Language::Chinese;
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
        ],
    )
    .persistent()
    .padding(Edges::all(8.0))
    .gap(4.0)
}

fn project_split<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let palette = theme.palette;
    let inner = theme.metrics.corner_xs;
    let project_font = theme.font(Level::Label);
    let project = state
        .io
        .project
        .as_deref()
        .unwrap_or(grana_i18n::common::untitled(ctx.language()))
        .to_string();
    let dirty = state.io.dirty && state.access.author();
    let full = ctx
        .measure(&project, TextStyle::new(project_font, Color::WHITE))
        .x;
    let open_label = grana_i18n::header::open(ctx.language());
    let inset = (theme.metrics.control_height * 0.44).round() + 1.0;
    let open_lead_w =
        ctx.measure(
            open_label,
            TextStyle::new(theme.button_font(), Color::WHITE),
        )
        .x + inset * 2.0
            + 20.0;
    let open_split_w = open_lead_w + theme.metrics.group_gap + theme.metrics.control_height;
    let available_lead =
        (crate::panel::TOOLBAR_LEFT - 12.0 - 30.0 - 12.0 - open_split_w - 28.0).max(84.0);
    let lead_width = available_lead;
    let measured = (lead_width - 34.0 - if dirty { 12.0 } else { 0.0 }).max(32.0);
    let menu_key = Key::new("project.menu");
    let open = ctx.popup_open(menu_key);
    let title = project.clone();
    let language = ctx.language();
    let read_only = state.access.viewer();
    let tooltip: Option<String> = if read_only {
        Some(grana_i18n::app::read_only_detail(language).to_string())
    } else {
        (full > measured).then(|| project.clone())
    };
    let mark = if read_only { Icon::Lock } else { Icon::Cube };
    let lead_element = grana_ui::canvas(Length::Fixed(lead_width), theme.metrics.control_height)
        .view((title.clone(), dirty), move |_, painter, info| {
            let frame = info.rect;
            let corners = Corners::split_leading(inner)
                .resolved(frame.size())
                .lerp(Corners::all(theme.metrics.corner_sm), info.press);
            let fill = palette
                .surface_container_high
                .lerp(palette.surface_container_highest, info.hover)
                .lerp(palette.on_surface, 0.08 * info.press);
            painter.rect(frame, corners, fill);
            icon::paint(
                mark,
                painter,
                Vec2::new(frame.min.x + 13.0, frame.center().y),
                13.0,
                palette.primary,
                1.6,
            );
            let title_left = frame.min.x + 23.0;
            painter.text_in(
                Rect::from_min_max(
                    Vec2::new(title_left, frame.min.y),
                    Vec2::new(title_left + measured, frame.max.y),
                ),
                &title,
                TextStyle::new(project_font, palette.on_surface).truncating(),
                Align::Center,
            );
            if dirty {
                painter.rect(
                    Rect::from_center_size(
                        Vec2::new(title_left + measured + 7.0, frame.center().y),
                        Vec2::splat(6.0),
                    ),
                    Corners::capsule(),
                    palette.tertiary,
                );
            }
        })
        .named("project.save")
        .focus_ring(Corners::capsule());
    let lead_element = if state.access.author() {
        lead_element.press(Message::Command(Command::SaveProject))
    } else {
        lead_element
    };
    let lead = match tooltip {
        Some(tooltip) => lead_element.tooltip(&tooltip),
        None => lead_element,
    };
    let trail_corners = if open {
        Corners::capsule()
    } else {
        Corners::split_trailing(inner)
    };
    let trail_icon = if open {
        Icon::ChevronUp
    } else {
        Icon::ChevronDown
    };
    let trail = grana_ui::widget::icon_button(
        trail_icon,
        26.0,
        if open { Role::Tonal } else { Role::SurfaceHigh },
        "",
    )
    .shaped(trail_corners)
    .key(menu_key)
    .press(Message::Noop);
    grana_ui::row()
        .gap(theme.metrics.group_gap)
        .align(Align::Center)
        .children([lead, trail])
}

fn open_split<'a>(ctx: &mut BuildContext<'_, Message>) -> Element<'a, Message> {
    let label = grana_i18n::header::open(ctx.language());
    crate::view::split_button(
        ctx,
        Some(Icon::Folder),
        label,
        Role::SurfaceHigh,
        ("open.lead", Message::Command(Command::OpenImage)),
        (Key::new("open.menu"), Message::Noop),
    )
}

fn actions<'a>(
    ctx: &mut BuildContext<'_, Message>,
    layout: &Layout,
    state: &'a View<'a>,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let inner = theme.metrics.corner_xs;
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
    let undo_element =
        grana_ui::widget::icon_button(Icon::Undo, 30.0, Role::SurfaceHigh, &undo_tooltip)
            .shaped(Corners::split_leading(inner))
            .named("undo")
            .enabled(undo.is_some())
            .press(Message::Command(Command::Undo));
    let redo_element =
        grana_ui::widget::icon_button(Icon::Redo, 30.0, Role::SurfaceHigh, &redo_tooltip)
            .shaped(Corners::split_trailing(inner))
            .named("redo")
            .enabled(redo.is_some())
            .press(Message::Command(Command::Redo));
    let history_group = grana_ui::row()
        .gap(theme.metrics.group_gap)
        .align(Align::Center)
        .children([undo_element, redo_element]);

    let sliced = state.camera.slice().is_some();
    let fit = crate::view::icon_button(
        ctx,
        "view.fit",
        Icon::Fit,
        30.0,
        Role::SurfaceHigh,
        grana_i18n::header::frame_volume(ctx.language()),
        Message::FitView,
    )
    .shaped(Corners::group_horizontal(
        0,
        3,
        Corners::CAPSULE,
        inner,
        false,
    ));
    let slice_btn = crate::view::icon_button(
        ctx,
        "view.slice.toggle",
        Icon::Section,
        30.0,
        if sliced {
            Role::Tonal
        } else {
            Role::SurfaceHigh
        },
        grana_i18n::inspector::slice_view(ctx.language()),
        Message::ToggleSlice(!sliced),
    )
    .shaped(Corners::group_horizontal(
        1,
        3,
        Corners::CAPSULE,
        inner,
        sliced,
    ));
    let settings_open = ctx.popup_open(Key::new("settings.menu"));
    let settings = crate::view::icon_button(
        ctx,
        "settings.menu",
        Icon::Sliders,
        30.0,
        if settings_open {
            Role::Tonal
        } else {
            Role::SurfaceHigh
        },
        grana_i18n::header::settings(ctx.language()),
        Message::Noop,
    )
    .shaped(Corners::group_horizontal(
        2,
        3,
        Corners::CAPSULE,
        inner,
        settings_open,
    ));
    let tools_group = grana_ui::row()
        .gap(theme.metrics.group_gap)
        .align(Align::Center)
        .children([fit, slice_btn, settings]);

    let settings_popup = settings_menu(ctx);
    let pending = state.io.pending > 0 || !state.jobs.is_empty();
    let mut children = Vec::new();
    if pending {
        children.push(crate::view::spinner(ctx, 18.0));
    }
    children.extend([
        palette_button(ctx),
        history_group,
        tools_group,
        analysis(ctx, state),
        settings_popup,
    ]);
    let surface = ctx.theme().surface(Role::FloatingToolbar, State::Rest);
    grana_ui::row()
        .at(layout.toolbar_right)
        .named("header.actions")
        .padding(Edges::symmetric(6.0, 6.0))
        .gap(6.0)
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
        30.0,
        Role::SurfaceHigh,
        &tooltip,
        Message::OpenPalette,
    )
}

fn analysis<'a>(ctx: &mut BuildContext<'_, Message>, state: &'a View<'a>) -> Element<'a, Message> {
    let open = state.workspace.panel.is_some();
    let role = if open {
        Role::Filled
    } else {
        Role::PrimaryContainer
    };
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
        return grana_ui::popover::popup(key, 240.0, std::iter::empty());
    }
    let mut rows = vec![
        command_row(ctx, "open.image"),
        command_row(ctx, "open.tiles"),
        command_row(ctx, "open.folder"),
        command_row(ctx, "open.remote"),
        menu_separator(ctx),
        command_row(ctx, "open.project"),
    ];
    if state.access.viewer() {
        rows.remove(1);
    }
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
    menu(ctx, key, 240.0, rows)
}

fn project_menu<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
) -> Element<'a, Message> {
    let key = Key::new("project.menu");
    if !ctx.popup_open(key) {
        return grana_ui::popover::popup(key, 224.0, std::iter::empty());
    }
    let mut rows = Vec::new();
    if state.access.author() {
        rows.push(command_row(ctx, "save.project"));
        rows.push(command_row(ctx, "save.as"));
        rows.push(menu_separator(ctx));
    }
    rows.push(command_row(ctx, "snapshot"));
    rows.push(command_row(ctx, "figure"));
    rows.push(command_row(ctx, "labels"));
    menu(ctx, key, 224.0, rows)
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
