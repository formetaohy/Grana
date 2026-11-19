use glam::Vec2;
use grana_session::Command;
use grana_ui::{
    Align, BuildContext, Color, Corners, Edges, Element, Icon, IconStyle, Key, Length, Level, Rect,
    Role, State, TextStyle, icon,
};

use crate::chrome::{CONTROLS_WIDTH, ChromeView};
use crate::message::Message;
use crate::panel::{GAP, Layout, MARGIN, TOOLBAR_RIGHT};
use crate::state::View;
use crate::view::menu;

const PADDING: f32 = 6.0;
const ITEM_GAP: f32 = 6.0;
const MINIMUM_LEAD: f32 = 112.0;
const LEAD_INSET: f32 = 34.0;

pub(super) struct Project {
    pub name: String,
    pub dirty: bool,
    pub read_only: bool,
    pub author: bool,
}

pub(super) fn project(state: &View<'_>, language: grana_i18n::Language) -> Project {
    Project {
        name: state
            .io
            .name
            .clone()
            .unwrap_or_else(|| grana_i18n::common::untitled(language).to_string()),
        dirty: state.io.dirty && state.access.author(),
        read_only: state.access.viewer(),
        author: state.access.author(),
    }
}

pub(super) struct Navigation {
    pub extent: f32,
    pub lead: f32,
}

pub(super) fn navigation_metrics(
    ctx: &mut BuildContext<'_, Message>,
    project: &Project,
    chrome: ChromeView,
    full: Rect,
) -> Navigation {
    let theme = *ctx.theme();
    let title = ctx
        .measure(
            &project.name,
            TextStyle::new(theme.font(Level::Label), Color::WHITE),
        )
        .x;
    let controls = if chrome.controls {
        CONTROLS_WIDTH + GAP
    } else {
        0.0
    };
    let available = full.width() - 2.0 * MARGIN - chrome.leading - controls - TOOLBAR_RIGHT - GAP;
    let fixed = 2.0 * PADDING
        + theme.metrics.control_height
        + ITEM_GAP
        + theme.metrics.group_gap
        + theme.metrics.control_height;
    let natural = title + LEAD_INSET + if project.dirty { 12.0 } else { 0.0 };
    let lead = natural
        .max(MINIMUM_LEAD)
        .min((available - fixed).max(MINIMUM_LEAD));
    Navigation {
        extent: fixed + lead,
        lead,
    }
}

pub(super) fn view<'a>(
    ctx: &mut BuildContext<'_, Message>,
    layout: &Layout,
    state: &'a View<'a>,
) -> Vec<Element<'a, Message>> {
    let mut elements = navigation(ctx, layout, state);
    elements.push(actions(ctx, layout, state));
    elements
}

fn navigation<'a>(
    ctx: &mut BuildContext<'_, Message>,
    layout: &Layout,
    state: &'a View<'a>,
) -> Vec<Element<'a, Message>> {
    let theme = *ctx.theme();
    let hidden = state.workspace.sidebar_hidden;
    let language = ctx.language();
    let tooltip = if hidden {
        grana_i18n::header::show_sidebar(language)
    } else {
        grana_i18n::header::hide_sidebar(language)
    };
    let toggle = crate::view::icon_button(
        ctx,
        "sidebar.toggle",
        Icon::Sidebar,
        30.0,
        if hidden {
            Role::SurfaceHigh
        } else {
            Role::Tonal
        },
        tooltip,
        Message::ToggleSidebar,
    )
    .shaped(Corners::capsule());
    let project = project(state, language);
    let split = project_split(ctx, layout, &project);
    let surface = theme.surface(Role::FloatingToolbar, State::Rest);
    let row = grana_ui::row()
        .at(layout.toolbar_left)
        .named("header.navigation")
        .padding(Edges::symmetric(PADDING, PADDING))
        .gap(ITEM_GAP)
        .align(Align::Center)
        .height(Length::Fill)
        .background(surface)
        .children([toggle, split]);
    vec![row, project_menu(ctx, state)]
}

fn project_split<'a>(
    ctx: &mut BuildContext<'_, Message>,
    layout: &Layout,
    project: &Project,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let palette = theme.palette;
    let inner = theme.metrics.corner_xs;
    let project_font = theme.font(Level::Label);
    let dirty = project.dirty;
    let full = ctx
        .measure(&project.name, TextStyle::new(project_font, Color::WHITE))
        .x;
    let lead_width = layout.project_lead;
    let measured = (lead_width - LEAD_INSET - if dirty { 12.0 } else { 0.0 }).max(32.0);
    let menu_key = Key::new("project.menu");
    let open = ctx.popup_open(menu_key);
    let title = project.name.clone();
    let language = ctx.language();
    let tooltip: Option<String> = if project.read_only {
        Some(grana_i18n::app::read_only_detail(language).to_string())
    } else {
        (full > measured).then(|| project.name.clone())
    };
    let mark = if project.read_only {
        Icon::Lock
    } else {
        Icon::Cube
    };
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
                Vec2::new(frame.min.x + 13.5, frame.center().y),
                15.0,
                palette.primary,
                IconStyle::REGULAR,
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
    let lead_element = if project.author {
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
        theme.metrics.control_height,
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
    let settings_open = state.workspace.settings.open;
    let settings = crate::view::icon_button(
        ctx,
        "header.settings",
        Icon::Sliders,
        30.0,
        if settings_open {
            Role::Tonal
        } else {
            Role::SurfaceHigh
        },
        grana_i18n::header::settings(ctx.language()),
        Message::ToggleSettings(!settings_open),
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
    ]);
    let surface = ctx.theme().surface(Role::FloatingToolbar, State::Rest);
    let actions = grana_ui::row()
        .named("header.actions")
        .padding(Edges::symmetric(6.0, 6.0))
        .gap(6.0)
        .align(Align::Center)
        .height(Length::Fill)
        .background(surface)
        .children(children);
    let band = Rect::from_min_max(
        Vec2::new(layout.toolbar_left.min.x, layout.toolbar_right.min.y),
        layout.toolbar_right.max,
    );
    grana_ui::row()
        .at(band)
        .named("header.actions.band")
        .align(Align::Center)
        .justify(Align::End)
        .height(Length::Fill)
        .children([actions])
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

fn project_menu<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
) -> Element<'a, Message> {
    let key = Key::new("project.menu");
    if !ctx.popup_open(key) {
        return grana_ui::popover::popup(key, 248.0, std::iter::empty());
    }
    let language = ctx.language();
    let mut rows = vec![
        command_row(ctx, "open.image"),
        command_row(ctx, "open.folder"),
        command_row(ctx, "open.project"),
    ];
    if state.access.author() {
        rows.push(command_row(ctx, "open.tiles"));
        rows.push(command_row(ctx, "package.open"));
    }
    rows.push(grana_ui::spacer(8.0));
    rows.push(crate::view::section_header(
        ctx,
        grana_i18n::header::open_remote(language),
        None,
    ));
    rows.push(command_row(ctx, "open.remote"));
    if !state.recent.is_empty() {
        rows.push(grana_ui::spacer(8.0));
        rows.push(crate::view::section_header(
            ctx,
            grana_i18n::header::recent(language),
            None,
        ));
        for (index, entry) in state.recent.iter().take(crate::RECENT_LIMIT).enumerate() {
            rows.push(crate::view::action_menu_message(
                ctx,
                &format!("project.recent.{index}"),
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
    if state.access.author() {
        rows.push(grana_ui::spacer(8.0));
        rows.push(crate::view::section_header(
            ctx,
            grana_i18n::header::save(language),
            None,
        ));
        rows.push(command_row(ctx, "save.project"));
        rows.push(command_row(ctx, "save.as"));
    }
    rows.push(grana_ui::spacer(8.0));
    rows.push(crate::view::section_header(
        ctx,
        grana_i18n::header::export(language),
        None,
    ));
    rows.push(command_row(ctx, "snapshot"));
    rows.push(command_row(ctx, "figure"));
    rows.push(command_row(ctx, "labels"));
    menu(ctx, key, 248.0, rows)
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
