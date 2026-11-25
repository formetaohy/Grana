use glam::Vec2;
use grana_session::Command;
use grana_ui::{
    Align, BuildContext, Color, Corners, Edges, Element, Icon, IconStyle, Key, Length, Level, Rect,
    Role, State, TextStyle, Theme, icon,
};

use crate::message::Message;
use crate::panel::{Layout, SIDEBAR_WIDTH};
use crate::state::View;
use crate::view::menu;

const PADDING: f32 = 6.0;
const ITEM_GAP: f32 = 6.0;
const TITLE_LEFT: f32 = 23.0;
const TITLE_TAIL: f32 = 6.0;
const DIRTY_TAIL: f32 = 16.0;
const DIRTY_DOT: f32 = 6.0;

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

pub(super) fn lead_extent(theme: &Theme) -> f32 {
    SIDEBAR_WIDTH
        - 2.0 * PADDING
        - 2.0 * ITEM_GAP
        - 2.0 * theme.metrics.control_height
        - theme.metrics.group_gap
        - theme.metrics.control_height
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
    let toggle_size = theme.metrics.control_height;
    let toggle = crate::view::icon_button(
        ctx,
        "sidebar.toggle",
        Icon::Sidebar,
        toggle_size,
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
    let split = project_split(ctx, &project);
    let settings_open = state.workspace.settings.open;
    let settings_size = theme.metrics.control_height;
    let settings = crate::view::icon_button(
        ctx,
        "header.settings",
        Icon::Sliders,
        settings_size,
        if settings_open {
            Role::Tonal
        } else {
            Role::SurfaceHigh
        },
        grana_i18n::header::settings(language),
        Message::SetSettings(if settings_open {
            None
        } else {
            Some(state.workspace.settings.tab)
        }),
    )
    .shaped(Corners::capsule());
    let surface = theme.surface(Role::FloatingToolbar, State::Rest);
    let row = grana_ui::row()
        .at(layout.toolbar_left)
        .named("header.navigation")
        .padding(Edges::symmetric(PADDING, PADDING))
        .gap(ITEM_GAP)
        .align(Align::Center)
        .height(Length::Fill)
        .background(surface)
        .children([toggle, split, settings]);
    vec![row, project_menu(ctx, state)]
}

fn project_split<'a>(
    ctx: &mut BuildContext<'_, Message>,
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
    let tail = if dirty { DIRTY_TAIL } else { TITLE_TAIL };
    let measured = lead_extent(&theme) - TITLE_LEFT - tail;
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
    let lead_element = grana_ui::canvas(Length::Fill, theme.metrics.control_height)
        .view(
            (title.clone(), dirty, full.to_bits()),
            move |_, painter, info| {
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
                let title_left = frame.min.x + TITLE_LEFT;
                let width = (frame.max.x - tail - title_left).max(8.0);
                painter.text_in(
                    Rect::from_min_max(
                        Vec2::new(title_left, frame.min.y),
                        Vec2::new(title_left + width, frame.max.y),
                    ),
                    &title,
                    TextStyle::new(project_font, palette.on_surface).truncating(),
                    Align::Center,
                );
                if dirty {
                    painter.rect(
                        Rect::from_center_size(
                            Vec2::new(
                                (title_left + full.min(width) + 7.0).min(frame.max.x - 9.0),
                                frame.center().y,
                            ),
                            Vec2::splat(DIRTY_DOT),
                        ),
                        Corners::capsule(),
                        palette.tertiary,
                    );
                }
            },
        )
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
        .width(Length::Fill)
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
        2,
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
        2,
        Corners::CAPSULE,
        inner,
        sliced,
    ));
    let tools_group = grana_ui::row()
        .gap(theme.metrics.group_gap)
        .align(Align::Center)
        .children([fit, slice_btn]);

    let pending = state.io.pending > 0 || !state.jobs.is_empty();
    let mut children = Vec::new();
    if pending {
        children.push(crate::view::loading(ctx));
    }
    if let Some(network) = transfer(ctx, state) {
        children.push(network);
    }
    children.extend([history_group, tools_group, analysis(ctx, state)]);
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

fn transfer<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &View<'a>,
) -> Option<Element<'a, Message>> {
    let network = state.io.network?;
    let language = ctx.language();
    let theme = *ctx.theme();
    let fetched = bytes(network.fetched as f64);
    let resumed = bytes(network.resumed as f64);
    let rate = network.rate.map_or_else(
        || fetched.clone(),
        |rate| format!("{}/s", bytes(rate as f64)),
    );
    let label = if network.resumed > 0 {
        grana_i18n::header::network_transfer(language, &rate, &resumed)
    } else {
        rate
    };
    let detail = grana_i18n::header::network_detail(language, &fetched, &resumed);
    let palette = theme.palette;
    Some(
        grana_ui::row()
            .named("header.transfer")
            .gap(6.0)
            .padding(Edges::symmetric(10.0, 0.0))
            .height(Length::Fixed(theme.metrics.control_height))
            .align(Align::Center)
            .background(theme.surface(Role::SurfaceHigh, State::Rest))
            .shaped(Corners::capsule())
            .tooltip(&detail)
            .children([
                grana_ui::widget::icon(
                    Icon::Globe,
                    15.0,
                    palette.on_surface_variant,
                    IconStyle::REGULAR,
                ),
                grana_ui::text(
                    label,
                    TextStyle::new(theme.font(Level::Caption), palette.on_surface_variant),
                ),
            ]),
    )
}

fn bytes(value: f64) -> String {
    const SCALE: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
    let mut value = value.max(0.0);
    let mut index = 0;
    while value >= 1024.0 && index + 1 < SCALE.len() {
        value /= 1024.0;
        index += 1;
    }
    if index == 0 {
        format!("{value:.0} {}", SCALE[index])
    } else {
        format!("{value:.1} {}", SCALE[index])
    }
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
        return grana_ui::popover::popup(key, 240.0, std::iter::empty());
    }
    let mut rows = vec![
        command_row(ctx, "open.image"),
        command_row(ctx, "open.folder"),
        command_row(ctx, "open.remote"),
        command_row(ctx, "open.project"),
    ];
    if state.access.author() {
        rows.push(command_row(ctx, "open.tiles"));
        rows.push(command_row(ctx, "package.open"));
    }
    if !state.recent.is_empty() {
        rows.push(grana_ui::spacer(6.0));
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
        rows.push(grana_ui::spacer(6.0));
        rows.push(command_row(ctx, "save.project"));
        rows.push(command_row(ctx, "save.as"));
    }
    rows.push(grana_ui::spacer(6.0));
    rows.push(command_row(ctx, "snapshot"));
    rows.push(command_row(ctx, "figure"));
    rows.push(command_row(ctx, "labels"));
    menu(ctx, key, 240.0, rows)
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
