use glam::Vec2;
use grana_session::Command;
use grana_ui::glass::GlassStyle;
use grana_ui::{
    Align, BuildContext, Color, Corners, Element, Icon, Key, Length, Level, Rect, TextStyle, icon,
};

use crate::message::Message;
use crate::panel::Layout;
use crate::state::View;
use crate::view::{action_menu_row, menu, menu_separator};

const COMMAND: &str = if cfg!(target_os = "macos") {
    "⌘"
} else {
    "Ctrl "
};

pub(super) fn view<'a>(
    ctx: &mut BuildContext<'_, Message>,
    layout: &Layout,
    state: &'a View<'a>,
) -> Vec<Element<'a, Message>> {
    vec![
        navigation(ctx, layout, state),
        actions(ctx, layout, state),
        analysis(ctx, layout, state),
    ]
}

fn navigation<'a>(
    ctx: &mut BuildContext<'_, Message>,
    layout: &Layout,
    state: &'a View<'a>,
) -> Element<'a, Message> {
    let group = Key::new("header.navigation.group");
    let hidden = state.workspace.sidebar_hidden;
    let tooltip = if hidden {
        "Show sidebar"
    } else {
        "Hide sidebar"
    };
    let toggle = crate::view::glass_icon_button(
        ctx,
        "sidebar.toggle",
        Icon::Sidebar,
        36.0,
        GlassStyle::button(),
        tooltip,
        Message::ToggleSidebar,
    )
    .glass_group(group);
    let project = project_pill(ctx, state, group);
    let open = open_pill(ctx, group);
    let project_menu = project_menu(ctx);
    let open_menu = open_menu(ctx);
    grana_ui::row()
        .at(layout.toolbar_left)
        .named("header.navigation")
        .gap(8.0)
        .align(Align::Center)
        .height(Length::Fill)
        .children([toggle, project, open, project_menu, open_menu])
}

fn project_pill<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
    group: Key,
) -> Element<'a, Message> {
    let palette = ctx.theme().palette;
    let brand_font = ctx.theme().font(Level::Callout).semibold();
    let project_font = ctx.theme().font(Level::Callout).medium();
    let project = state
        .io
        .project
        .as_deref()
        .unwrap_or("Untitled")
        .to_string();
    let dirty = state.io.dirty;
    let measured = ctx
        .measure(&project, TextStyle::new(project_font, Color::WHITE))
        .x
        .min(136.0);
    let width = 80.0 + 10.0 + measured + if dirty { 30.0 } else { 22.0 };
    let key = Key::new("project.menu");
    let open = ctx.popup_open(key);
    let title = project.clone();
    grana_ui::canvas(Length::Fixed(width), 36.0, move |painter, info| {
        let frame = info.glass.unwrap_or(info.rect);
        let engaged = info.hover.max(if open { 1.0 } else { 0.0 });
        if engaged > 0.01 {
            painter.rect(
                frame,
                Corners::capsule(),
                palette.material_fill.amplify(0.6 * engaged),
            );
        }
        let widths = [6.5, 7.6, 8.7, 9.8, 10.9];
        let mut y = frame.center().y - 7.4;
        for bar_width in widths {
            painter.rect(
                Rect::from_min_size(Vec2::new(frame.min.x + 12.0, y), Vec2::new(bar_width, 2.0)),
                Corners::capsule(),
                palette.brand,
            );
            y += 3.2;
        }
        painter.text_in(
            Rect::from_min_size(
                Vec2::new(frame.min.x + 27.0, frame.min.y),
                Vec2::new(46.0, frame.height()),
            ),
            "Grana",
            TextStyle::new(brand_font, palette.text_primary),
            Align::Center,
        );
        painter.rect(
            Rect::from_center_size(
                Vec2::new(frame.min.x + 78.0, frame.center().y),
                Vec2::new(1.0, 14.0),
            ),
            Corners::ZERO,
            palette.separator,
        );
        let title_left = frame.min.x + 87.0;
        painter.text_in(
            Rect::from_min_max(
                Vec2::new(title_left, frame.min.y),
                Vec2::new(title_left + measured + 2.0, frame.max.y),
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
                    Vec2::new(title_left + measured + 8.0, frame.center().y),
                    Vec2::splat(5.0),
                ),
                Corners::capsule(),
                palette.warning,
            );
        }
        icon::paint(
            Icon::ChevronDown,
            painter,
            Vec2::new(frame.max.x - 11.0, frame.center().y),
            9.0,
            palette.text_tertiary,
            1.5,
        );
    })
    .key(key)
    .press(Message::Noop)
    .glass(GlassStyle::button().interactive())
    .glass_group(group)
    .focus_ring(Corners::capsule())
}

fn open_pill<'a>(ctx: &mut BuildContext<'_, Message>, group: Key) -> Element<'a, Message> {
    let palette = ctx.theme().palette;
    let font = ctx.theme().font(Level::Callout).medium();
    let key = Key::new("open.menu");
    let open = ctx.popup_open(key);
    grana_ui::canvas(Length::Fixed(84.0), 36.0, move |painter, info| {
        let frame = info.glass.unwrap_or(info.rect);
        let engaged = info.hover.max(if open { 1.0 } else { 0.0 });
        if engaged > 0.01 {
            painter.rect(
                frame,
                Corners::capsule(),
                palette.material_fill.amplify(0.6 * engaged),
            );
        }
        let color = palette.text_secondary.lerp(
            palette.text_primary,
            info.hover.max(if open { 1.0 } else { 0.0 }),
        );
        icon::paint(
            Icon::Folder,
            painter,
            Vec2::new(frame.min.x + 15.0, frame.center().y),
            13.0,
            color,
            1.5,
        );
        painter.text_in(
            Rect::from_min_max(
                Vec2::new(frame.min.x + 26.0, frame.min.y),
                Vec2::new(frame.max.x - 18.0, frame.max.y),
            ),
            "Open",
            TextStyle::new(font, color),
            Align::Center,
        );
        icon::paint(
            Icon::ChevronDown,
            painter,
            Vec2::new(frame.max.x - 11.0, frame.center().y),
            9.0,
            palette.text_tertiary,
            1.5,
        );
    })
    .key(key)
    .press(Message::Noop)
    .glass(GlassStyle::button().interactive())
    .glass_group(group)
    .focus_ring(Corners::capsule())
}

fn actions<'a>(
    ctx: &mut BuildContext<'_, Message>,
    layout: &Layout,
    state: &'a View<'a>,
) -> Element<'a, Message> {
    let group = Key::new("header.actions.group");
    let undo = state.history.undo;
    let redo = state.history.redo;
    let undo_tooltip = undo.map_or_else(
        || "Nothing to undo".to_string(),
        |action| format!("Undo {}", action.label()),
    );
    let redo_tooltip = redo.map_or_else(
        || "Nothing to redo".to_string(),
        |action| format!("Redo {}", action.label()),
    );
    let undo_element = if undo.is_some() {
        crate::view::glass_icon_button(
            ctx,
            "undo",
            Icon::Undo,
            36.0,
            GlassStyle::button(),
            &undo_tooltip,
            Message::Command(Command::Undo),
        )
    } else {
        crate::view::glass_icon_button_disabled(
            ctx,
            "undo",
            Icon::Undo,
            36.0,
            GlassStyle::button_disabled(),
        )
    }
    .glass_group(group);
    let redo_element = if redo.is_some() {
        crate::view::glass_icon_button(
            ctx,
            "redo",
            Icon::Redo,
            36.0,
            GlassStyle::button(),
            &redo_tooltip,
            Message::Command(Command::Redo),
        )
    } else {
        crate::view::glass_icon_button_disabled(
            ctx,
            "redo",
            Icon::Redo,
            36.0,
            GlassStyle::button_disabled(),
        )
    }
    .glass_group(group);
    let palette = ctx.theme().palette;
    let pending = state.io.pending > 0 || !state.jobs.is_empty();
    let diagnostics = crate::view::glass_icon_button(
        ctx,
        "header.diagnostics",
        Icon::Info,
        36.0,
        GlassStyle::button(),
        "Streaming diagnostics",
        Message::Noop,
    )
    .glass_group(group);
    let mut children = vec![
        undo_element,
        redo_element,
        diagnostics,
        diagnostics_menu(ctx, state),
    ];
    if pending {
        let dot_rect = Rect::from_center_size(
            Vec2::new(
                layout.toolbar_right.max.x - 7.0,
                layout.toolbar_right.min.y + 7.0,
            ),
            Vec2::splat(5.0),
        );
        children.push(
            grana_ui::canvas(Length::Fixed(5.0), 5.0, move |painter, info| {
                painter.rect(info.rect, Corners::capsule(), palette.accent);
            })
            .named("header.pending")
            .at(dot_rect),
        );
    }
    grana_ui::row()
        .at(layout.toolbar_right)
        .named("header.actions")
        .gap(8.0)
        .align(Align::Center)
        .justify(Align::End)
        .width(Length::Fill)
        .height(Length::Fill)
        .children(children)
}

fn analysis<'a>(
    ctx: &mut BuildContext<'_, Message>,
    layout: &Layout,
    state: &'a View<'a>,
) -> Element<'a, Message> {
    let palette = ctx.theme().palette;
    let font = ctx.theme().button_font();
    let open = state.workspace.panel.is_some();
    let style = if open {
        GlassStyle::primary_active()
    } else {
        GlassStyle::primary()
    };
    let message = if open {
        Message::SetPanel(None)
    } else {
        Message::SetPanel(Some(state.workspace.last_panel))
    };
    grana_ui::canvas(Length::Fill, 36.0, move |painter, info| {
        let frame = info.glass.unwrap_or(info.rect);
        let mark = Rect::from_center_size(
            Vec2::new(frame.min.x + 22.0, frame.center().y),
            Vec2::splat(13.0),
        );
        icon::paint(Icon::Table, painter, mark.center(), 13.0, Color::WHITE, 1.6);
        painter.text_in(
            Rect::from_min_max(
                Vec2::new(mark.max.x + 7.0, frame.min.y),
                Vec2::new(frame.max.x - 14.0, frame.max.y),
            ),
            "Analysis",
            TextStyle::new(font, Color::WHITE).aligned(Align::Start),
            Align::Center,
        );
        if open {
            painter.rect(
                Rect::from_center_size(
                    Vec2::new(frame.max.x - 12.0, frame.center().y),
                    Vec2::splat(4.5),
                ),
                Corners::capsule(),
                palette.text_on_accent.with_alpha(0.85),
            );
        }
    })
    .at(layout.analysis)
    .named("header.analysis")
    .glass(style.interactive())
    .press(message)
    .focus_ring(Corners::capsule())
}

fn open_menu<'a>(ctx: &mut BuildContext<'_, Message>) -> Element<'a, Message> {
    let key = Key::new("open.menu");
    if !ctx.popup_open(key) {
        return grana_ui::popover::popup(key, 236.0, std::iter::empty());
    }
    let shortcut = format!("{COMMAND}O");
    let rows = vec![
        action_menu_row(
            ctx,
            "open.image",
            Icon::Cube,
            "Open Image…",
            None,
            Command::OpenImage,
        ),
        action_menu_row(
            ctx,
            "open.folder",
            Icon::Folder,
            "Open OME-Zarr Folder…",
            None,
            Command::OpenDataset,
        ),
        crate::view::action_menu_message(
            ctx,
            "open.remote",
            Icon::Globe,
            "Open Remote OME-Zarr…",
            Message::BeginRemote,
        ),
        menu_separator(ctx),
        action_menu_row(
            ctx,
            "open.project",
            Icon::Folder,
            "Open Project…",
            Some(&shortcut),
            Command::OpenProject,
        ),
    ];
    menu(ctx, key, 236.0, rows)
}

fn project_menu<'a>(ctx: &mut BuildContext<'_, Message>) -> Element<'a, Message> {
    let key = Key::new("project.menu");
    if !ctx.popup_open(key) {
        return grana_ui::popover::popup(key, 228.0, std::iter::empty());
    }
    let mut rows = Vec::new();
    let save = format!("{COMMAND}S");
    rows.push(action_menu_row(
        ctx,
        "save.project",
        Icon::Save,
        "Save Project",
        Some(&save),
        Command::SaveProject,
    ));
    rows.push(action_menu_row(
        ctx,
        "save.as",
        Icon::Save,
        "Save Project As…",
        None,
        Command::SaveProjectAs,
    ));
    rows.push(menu_separator(ctx));
    rows.push(action_menu_row(
        ctx,
        "snapshot",
        Icon::Camera,
        "Save Snapshot…",
        None,
        Command::SaveSnapshot,
    ));
    rows.push(action_menu_row(
        ctx,
        "labels",
        Icon::Export,
        "Export Labels Only…",
        None,
        Command::ExportAllLabels,
    ));
    menu(ctx, key, 228.0, rows)
}

fn diagnostics_menu<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
) -> Element<'a, Message> {
    let key = Key::new("header.diagnostics");
    if !ctx.popup_open(key) {
        return grana_ui::popover::popup(key, 304.0, std::iter::empty());
    }
    let frame = state.frame;
    let mut rows = Vec::new();
    rows.push(crate::view::section_header(
        ctx,
        "Streaming",
        Some(state.io.adapter),
    ));
    rows.push(crate::view::labelled(ctx, "Volume", &bricks(frame.volume)));
    rows.push(crate::view::labelled(
        ctx,
        "Section",
        &bricks(frame.section),
    ));
    rows.push(crate::view::labelled(
        ctx,
        "Levels",
        &format!(
            "{} · {} stored",
            state.scene.dataset.level_count(),
            state.scene.dataset.stored_levels()
        ),
    ));
    rows.push(crate::view::labelled(
        ctx,
        "Geometry",
        &format!(
            "{:.0} MiB · {:.0} MiB disk",
            state.scene.geometry_bytes() as f64 / (1024.0 * 1024.0),
            state.io.store.spilled as f64 / (1024.0 * 1024.0)
        ),
    ));
    rows.push(crate::view::labelled(
        ctx,
        "Bricks",
        &format!("{:.0} MiB", frame.bytes as f64 / (1024.0 * 1024.0)),
    ));
    rows.push(crate::view::labelled(
        ctx,
        "Transparent",
        &format!(
            "{:.0} MiB · {} layers · {} dropped",
            frame.fragments as f64 / (1024.0 * 1024.0),
            frame.oit_layers,
            frame.oit_dropped
        ),
    ));
    if state.io.pending > 0 {
        rows.push(grana_ui::row().gap(8.0).align(Align::Center).children([
            crate::view::spinner(ctx, 12.0),
            crate::view::caption(ctx, &format!("{} bricks in flight", state.io.pending)),
        ]));
    }
    rows.push(menu_separator(ctx));
    rows.push(crate::view::section_header(ctx, "Frame", None));
    rows.push(crate::view::labelled(
        ctx,
        "Graphics",
        &frame_budget(frame.profile),
    ));
    for (label, millis) in frame
        .profile
        .rows
        .iter()
        .filter(|(_, millis)| *millis > 0.0)
    {
        rows.push(crate::view::labelled(
            ctx,
            label,
            &format!("{millis:.2} ms"),
        ));
    }
    rows.push(menu_separator(ctx));
    rows.push(
        crate::view::button_with(
            ctx,
            "Clear Derived Cache",
            grana_ui::Role::Danger,
            Length::Fill,
        )
        .named("cache.clear")
        .press(Message::Command(Command::ClearCache)),
    );
    menu(ctx, key, 304.0, rows)
}

fn bricks(view: crate::state::ViewStats) -> String {
    if view.demand == view.visible {
        format!("L{} · {}/{}", view.level + 1, view.resident, view.demand)
    } else {
        format!(
            "L{} · {}/{} of {}",
            view.level + 1,
            view.resident,
            view.demand,
            view.visible
        )
    }
}

fn frame_budget(profile: crate::state::ProfileStats) -> String {
    if !profile.measured {
        return "measuring".to_string();
    }
    let marker = if profile.over_budget() { "over" } else { "of" };
    format!(
        "{:.2} ms {marker} {:.0} ms · ×{:.2}",
        profile.total, profile.target, profile.penalty
    )
}
