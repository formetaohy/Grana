use glam::Vec2;
use grana_ui::glass::{CHROME, GlassStyle};
use grana_ui::{
    Align, Block, Color, Corners, GlassGroup, Icon, Length, Level, Motion, Rect, Role, TextStyle,
    Ui, icon,
};

use crate::panel::Layout;
use crate::state::{State, ViewStats};
use crate::widget;
use grana_session::Command;

const COMMAND: &str = if cfg!(target_os = "macos") {
    "⌘"
} else {
    "Ctrl "
};

pub(super) fn bar(
    ui: &mut Ui,
    layout: &Layout,
    state: &mut State<'_>,
    commands: &mut Vec<Command>,
) {
    let left = ui.glass_group(ui.id("header.navigation.group"), 0.0, CHROME);
    let right = ui.glass_group(ui.id("header.actions.group"), 0.0, CHROME);
    navigation(ui, layout.toolbar_left, state, commands, left);
    actions(ui, layout.toolbar_right, state, commands, right);
    analysis(ui, layout.analysis, state);
}

fn navigation(
    ui: &mut Ui,
    rect: Rect,
    state: &mut State<'_>,
    commands: &mut Vec<Command>,
    group: GlassGroup,
) {
    ui.place(rect, |ui| {
        ui.row(
            Block::row()
                .gaps(8.0)
                .align(Align::Center)
                .height(Length::Fill),
            |ui| {
                let hidden = state.workspace.sidebar_hidden;
                let response = widget::glass_icon_button_response(
                    ui,
                    Some(group),
                    "sidebar.toggle",
                    Icon::Sidebar,
                    36.0,
                    GlassStyle::button(),
                    if hidden {
                        "Show sidebar"
                    } else {
                        "Hide sidebar"
                    },
                );
                if response.clicked {
                    state.workspace.sidebar_hidden = !hidden;
                }
                let project = project_pill(
                    ui,
                    Some(group),
                    state.io.project.as_deref().unwrap_or("Untitled"),
                    state.io.dirty,
                );
                project_menu(ui, state, commands, project);
                let open = open_pill(ui, Some(group));
                open_menu(ui, state, commands, open);
            },
        );
    });
}

fn project_pill(ui: &mut Ui, group: Option<GlassGroup>, project: &str, dirty: bool) -> Rect {
    let key = ui.id("project.menu");
    let palette = ui.theme().palette;
    let brand_font = ui.theme().font(Level::Callout).semibold();
    let project_font = ui.theme().font(Level::Callout).medium();
    let measured = ui
        .measure_text(project, TextStyle::new(project_font, Color::WHITE))
        .x
        .min(136.0);
    let width = 80.0 + 10.0 + measured + if dirty { 30.0 } else { 22.0 };
    let rect = ui.allocate(Vec2::new(width, 36.0));
    let mut response = ui.interact(key, rect);
    if ui.focusable(key, rect, Corners::capsule()) && ui.activated(key) {
        response.clicked = true;
    }
    if response.clicked {
        ui.toggle_popup(key);
    }
    let active = ui.popup_open(key);
    let hover = ui.animate(
        key,
        if response.hovered || active { 1.0 } else { 0.0 },
        Motion::quick(),
    );
    let mut style = GlassStyle::button().interactive();
    if response.hovered || response.active || active {
        style.material.specular = (style.material.specular + 0.14).min(1.0);
        style.material.rim += 0.05;
    }
    ui.glass_in(group, key, rect, style);
    let frame = ui.glass_frame(key).unwrap_or(rect);
    let mut painter = ui.painter();
    let widths = [6.5, 7.6, 8.7, 9.8, 10.9];
    let mut y = frame.center().y - 7.4;
    for bar_w in widths {
        painter.rect(
            Rect::from_min_size(Vec2::new(frame.min.x + 12.0, y), Vec2::new(bar_w, 2.0)),
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
        "grana",
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
        project,
        TextStyle::new(
            project_font,
            palette.text_secondary.lerp(palette.text_primary, hover),
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
        &mut painter,
        Vec2::new(frame.max.x - 11.0, frame.center().y),
        9.0,
        palette.text_tertiary,
        1.5,
    );
    rect
}

fn open_pill(ui: &mut Ui, group: Option<GlassGroup>) -> Rect {
    let key = ui.id("open.menu");
    let palette = ui.theme().palette;
    let font = ui.theme().font(Level::Callout).medium();
    let rect = ui.allocate(Vec2::new(84.0, 36.0));
    let mut response = ui.interact(key, rect);
    if ui.focusable(key, rect, Corners::capsule()) && ui.activated(key) {
        response.clicked = true;
    }
    if response.clicked {
        ui.toggle_popup(key);
    }
    let active = ui.popup_open(key);
    let hover = ui.animate(
        key,
        if response.hovered || active { 1.0 } else { 0.0 },
        Motion::quick(),
    );
    let mut style = GlassStyle::button().interactive();
    if response.hovered || response.active || active {
        style.material.specular = (style.material.specular + 0.14).min(1.0);
        style.material.rim += 0.05;
    }
    ui.glass_in(group, key, rect, style);
    let frame = ui.glass_frame(key).unwrap_or(rect);
    let color = palette.text_secondary.lerp(palette.text_primary, hover);
    let mut painter = ui.painter();
    icon::paint(
        Icon::Folder,
        &mut painter,
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
        &mut painter,
        Vec2::new(frame.max.x - 11.0, frame.center().y),
        9.0,
        palette.text_tertiary,
        1.5,
    );
    rect
}

fn actions(
    ui: &mut Ui,
    rect: Rect,
    state: &mut State<'_>,
    commands: &mut Vec<Command>,
    group: GlassGroup,
) {
    ui.place(rect, |ui| {
        ui.row(
            Block::row()
                .gaps(8.0)
                .align(Align::Center)
                .justify(Align::End)
                .width(Length::Fill)
                .height(Length::Fill),
            |ui| {
                let undo = state.history.undo;
                let redo = state.history.redo;
                let undo_tip = undo.map_or_else(
                    || "Nothing to undo".to_string(),
                    |action| format!("Undo {}", action.label()),
                );
                let redo_tip = redo.map_or_else(
                    || "Nothing to redo".to_string(),
                    |action| format!("Redo {}", action.label()),
                );
                let response = if undo.is_some() {
                    widget::glass_icon_button_response(
                        ui,
                        Some(group),
                        "undo",
                        Icon::Undo,
                        36.0,
                        GlassStyle::button(),
                        &undo_tip,
                    )
                } else {
                    widget::glass_icon_button_disabled(
                        ui,
                        Some(group),
                        "undo",
                        Icon::Undo,
                        36.0,
                        GlassStyle::button_disabled(),
                    )
                };
                if response.clicked {
                    commands.push(Command::Undo);
                }
                let response = if redo.is_some() {
                    widget::glass_icon_button_response(
                        ui,
                        Some(group),
                        "redo",
                        Icon::Redo,
                        36.0,
                        GlassStyle::button(),
                        &redo_tip,
                    )
                } else {
                    widget::glass_icon_button_disabled(
                        ui,
                        Some(group),
                        "redo",
                        Icon::Redo,
                        36.0,
                        GlassStyle::button_disabled(),
                    )
                };
                if response.clicked {
                    commands.push(Command::Redo);
                }
                let key = ui.id("header.diagnostics");
                let response = widget::glass_icon_button_response(
                    ui,
                    Some(group),
                    "header.diagnostics",
                    Icon::Info,
                    36.0,
                    GlassStyle::button(),
                    "Streaming diagnostics",
                );
                if state.io.pending > 0 || !state.jobs.is_empty() {
                    let accent = ui.theme().palette.accent;
                    let mut painter = ui.painter();
                    painter.rect(
                        Rect::from_center_size(
                            Vec2::new(response.rect.max.x - 7.0, response.rect.min.y + 7.0),
                            Vec2::splat(5.0),
                        ),
                        Corners::capsule(),
                        accent,
                    );
                }
                if response.clicked {
                    ui.toggle_popup(key);
                }
                diagnostics(ui, state, commands, response.rect);
            },
        );
    });
}

fn analysis(ui: &mut Ui, rect: Rect, state: &mut State<'_>) {
    let key = ui.id("analysis");
    let palette = ui.theme().palette;
    let mut response = ui.interact(key, rect);
    if ui.focusable(key, rect, Corners::capsule()) && ui.activated(key) {
        response.clicked = true;
    }
    let open = state.workspace.panel.is_some();
    let mut style = if open {
        GlassStyle::primary_active()
    } else {
        GlassStyle::primary()
    }
    .interactive();
    if response.hovered || response.active {
        style.material.specular = (style.material.specular + 0.14).min(1.0);
        style.material.rim += 0.05;
    }
    ui.glass(key, rect, style);
    let font = ui.theme().button_font();
    let frame = ui.glass_frame(key).unwrap_or(rect);
    {
        let mut painter = ui.painter();
        let mark = Rect::from_center_size(
            Vec2::new(frame.min.x + 22.0, frame.center().y),
            Vec2::splat(13.0),
        );
        icon::paint(
            Icon::Table,
            &mut painter,
            mark.center(),
            13.0,
            Color::WHITE,
            1.6,
        );
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
    }
    if response.clicked {
        state.workspace.panel = if open {
            None
        } else {
            let last = ui
                .value_of(ui.id("dock.last"))
                .map_or(0, |value| value.round().clamp(0.0, 4.0) as usize);
            Some(crate::panel::Panel::ALL[last])
        };
    }
}

fn open_menu(ui: &mut Ui, state: &State<'_>, commands: &mut Vec<Command>, rect: Rect) {
    let open = ui.popup_open(ui.id("open.menu"));
    let mut chosen = None;
    let busy = state.io.loading.is_some();
    grana_ui::popover::popover(ui, "open.menu", open, rect, 236.0, |ui| {
        if grana_ui::popover::action_row(ui, "open.image", Icon::Cube, "Open Image…", None) {
            chosen = Some(Command::OpenImage);
        }
        if grana_ui::popover::action_row(
            ui,
            "open.folder",
            Icon::Folder,
            "Open OME-Zarr Folder…",
            None,
        ) {
            chosen = Some(Command::OpenDataset);
        }
        grana_ui::popover::separator(ui);
        if grana_ui::popover::action_row(
            ui,
            "open.project",
            Icon::Folder,
            "Open Project…",
            Some(&format!("{COMMAND}O")),
        ) {
            chosen = Some(Command::OpenProject);
        }
    });
    if let Some(action) = chosen {
        ui.close_popup();
        if !busy {
            commands.push(action);
        }
    }
}

fn project_menu(ui: &mut Ui, state: &State<'_>, commands: &mut Vec<Command>, rect: Rect) {
    let open = ui.popup_open(ui.id("project.menu"));
    let mut chosen = None;
    let busy = state.io.loading.is_some();
    grana_ui::popover::popover(ui, "project.menu", open, rect, 228.0, |ui| {
        if grana_ui::popover::action_row(
            ui,
            "save.project",
            Icon::Save,
            "Save Project",
            Some(&format!("{COMMAND}S")),
        ) {
            chosen = Some(Command::SaveProject);
        }
        if grana_ui::popover::action_row(ui, "save.as", Icon::Save, "Save Project As…", None) {
            chosen = Some(Command::SaveProjectAs);
        }
        grana_ui::popover::separator(ui);
        if grana_ui::popover::action_row(ui, "snapshot", Icon::Camera, "Save Snapshot…", None) {
            chosen = Some(Command::SaveSnapshot);
        }
    });
    if let Some(action) = chosen {
        ui.close_popup();
        if !busy {
            commands.push(action);
        }
    }
}

fn diagnostics(ui: &mut Ui, state: &State<'_>, commands: &mut Vec<Command>, anchor: Rect) {
    let open = ui.popup_open(ui.id("header.diagnostics"));
    let mut clear = false;
    grana_ui::popover::popover(ui, "header.diagnostics", open, anchor, 304.0, |ui| {
        crate::panel::section_header(ui, "Streaming", Some(state.io.adapter));
        widget::labelled(ui, "Volume", &bricks(state.frame.volume));
        widget::labelled(ui, "Section", &bricks(state.frame.section));
        widget::labelled(
            ui,
            "Levels",
            &format!(
                "{} · {} stored",
                state.scene.dataset.level_count(),
                state.scene.dataset.stored_levels()
            ),
        );
        widget::labelled(
            ui,
            "Geometry",
            &format!(
                "{:.0} MiB · {:.0} MiB disk",
                state.scene.geometry_bytes() as f64 / (1024.0 * 1024.0),
                state.io.store.spilled as f64 / (1024.0 * 1024.0)
            ),
        );
        widget::labelled(
            ui,
            "Bricks",
            &format!("{:.0} MiB", state.frame.bytes as f64 / (1024.0 * 1024.0)),
        );
        widget::labelled(
            ui,
            "Transparent",
            &format!(
                "{:.0} MiB · {} layers · {} dropped",
                state.frame.fragments as f64 / (1024.0 * 1024.0),
                state.frame.oit_layers,
                state.frame.oit_dropped
            ),
        );
        if state.io.pending > 0 {
            ui.row(Block::row().gaps(8.0).align(Align::Center), |ui| {
                widget::spinner(ui, 12.0);
                widget::caption(ui, &format!("{} bricks in flight", state.io.pending));
            });
        }
        grana_ui::popover::separator(ui);
        if widget::button_with(
            ui,
            "cache.clear",
            "Clear Derived Cache",
            Role::Danger,
            Length::Fill,
        )
        .clicked
        {
            clear = true;
        }
    });
    if clear {
        ui.close_popup();
        commands.push(Command::ClearCache);
    }
}

fn bricks(view: ViewStats) -> String {
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
