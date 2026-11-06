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
    let left = ui.glass_group(ui.id("header.navigation.group"), 10.0, CHROME);
    let right = ui.glass_group(ui.id("header.actions.group"), 10.0, CHROME);
    let mut left_hover = Vec::new();
    let mut right_hover = Vec::new();
    navigation(
        ui,
        layout.toolbar_left,
        state,
        commands,
        left,
        &mut left_hover,
    );
    actions(
        ui,
        layout.toolbar_right,
        state,
        commands,
        right,
        &mut right_hover,
    );
    analysis(ui, layout.analysis, state);
    let pointer = ui.input().position();
    let left_spacing = ui.spring_to(
        ui.id("header.navigation.spacing"),
        if left_hover.iter().any(|rect| rect.contains(pointer)) {
            14.0
        } else {
            10.0
        },
        Motion::quick(),
    );
    ui.set_glass_group_spacing(left, left_spacing);
    let right_spacing = ui.spring_to(
        ui.id("header.actions.spacing"),
        if right_hover.iter().any(|rect| rect.contains(pointer)) {
            14.0
        } else {
            10.0
        },
        Motion::quick(),
    );
    ui.set_glass_group_spacing(right, right_spacing);
}

fn navigation(
    ui: &mut Ui,
    rect: Rect,
    state: &mut State<'_>,
    commands: &mut Vec<Command>,
    group: GlassGroup,
    hover: &mut Vec<Rect>,
) {
    let inner = rect.shrink2(Vec2::new(8.0, 0.0));
    ui.place(inner, |ui| {
        ui.row(
            Block::row()
                .gaps(6.0)
                .align(Align::Center)
                .height(Length::Fill),
            |ui| {
                let hidden = state.workspace.sidebar_hidden;
                let response = widget::glass_icon_button_response(
                    ui,
                    Some(group),
                    "sidebar.toggle",
                    Icon::Sidebar,
                    34.0,
                    GlassStyle::button(),
                    if hidden {
                        "Show sidebar"
                    } else {
                        "Hide sidebar"
                    },
                );
                hover.push(response.rect);
                if response.clicked {
                    state.workspace.sidebar_hidden = !hidden;
                }
                brand(ui, group);
                let open =
                    widget::glass_menu_button(ui, Some(group), "open.menu", "Open", false, 80.0);
                hover.push(open);
                open_menu(ui, state, commands, open);
                let project = widget::glass_menu_button(
                    ui,
                    Some(group),
                    "project.menu",
                    state.io.project.as_deref().unwrap_or("Untitled"),
                    state.io.dirty,
                    148.0,
                );
                hover.push(project);
                project_menu(ui, state, commands, project);
            },
        );
    });
}

fn actions(
    ui: &mut Ui,
    rect: Rect,
    state: &mut State<'_>,
    commands: &mut Vec<Command>,
    group: GlassGroup,
    hover: &mut Vec<Rect>,
) {
    let inner = rect.shrink2(Vec2::new(8.0, 0.0));
    ui.place(inner, |ui| {
        ui.row(
            Block::row()
                .gaps(6.0)
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
                        32.0,
                        GlassStyle::button(),
                        &undo_tip,
                    )
                } else {
                    widget::glass_icon_button_disabled(
                        ui,
                        Some(group),
                        "undo",
                        Icon::Undo,
                        32.0,
                        GlassStyle::button_disabled(),
                    )
                };
                hover.push(response.rect);
                if response.clicked {
                    commands.push(Command::Undo);
                }
                let response = if redo.is_some() {
                    widget::glass_icon_button_response(
                        ui,
                        Some(group),
                        "redo",
                        Icon::Redo,
                        32.0,
                        GlassStyle::button(),
                        &redo_tip,
                    )
                } else {
                    widget::glass_icon_button_disabled(
                        ui,
                        Some(group),
                        "redo",
                        Icon::Redo,
                        32.0,
                        GlassStyle::button_disabled(),
                    )
                };
                hover.push(response.rect);
                if response.clicked {
                    commands.push(Command::Redo);
                }
                let response = widget::glass_icon_button_response(
                    ui,
                    Some(group),
                    "fit",
                    Icon::Fit,
                    32.0,
                    GlassStyle::button(),
                    "Fit volume to view",
                );
                hover.push(response.rect);
                if response.clicked {
                    crate::view::fit_volume(state);
                }
                let key = ui.id("header.diagnostics");
                let response = widget::glass_icon_button_response(
                    ui,
                    Some(group),
                    "header.diagnostics",
                    Icon::Info,
                    32.0,
                    GlassStyle::button(),
                    "Streaming diagnostics",
                );
                hover.push(response.rect);
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
    let style = if open {
        GlassStyle::primary_active()
    } else {
        GlassStyle::primary()
    };
    ui.glass(key, rect, style.interactive());
    let hover = ui.animate(
        key ^ 0x41,
        if response.hovered { 1.0 } else { 0.0 },
        Motion::quick(),
    );
    let font = ui.theme().button_font();
    let frame = ui.glass_frame(key).unwrap_or(rect);
    {
        let mut painter = ui.painter();
        if hover > 0.01 {
            painter.outlined(
                frame.shrink(0.5),
                Corners::capsule(),
                grana_ui::Fill::None,
                grana_ui::Border {
                    width: 1.0,
                    color: Color::white_alpha(0.18 * hover),
                },
                grana_ui::Shadow::NONE,
            );
        }
        let mark = Rect::from_center_size(
            Vec2::new(frame.min.x + 24.0, frame.center().y),
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
                    Vec2::splat(4.0),
                ),
                Corners::capsule(),
                palette.text_on_accent.with_alpha(0.75),
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

fn brand(ui: &mut Ui, group: GlassGroup) {
    let rect = ui.allocate(Vec2::new(88.0, 34.0));
    ui.glass_in(
        Some(group),
        ui.id("header.brand"),
        rect,
        GlassStyle::button(),
    );
    let brand = ui.theme().palette.brand;
    let palette = ui.theme().palette;
    let font = ui.theme().font(Level::Title);
    let mut painter = ui.painter();
    let widths = [7.0, 8.2, 9.4, 10.6, 11.8];
    let mut y = rect.center().y - 8.0;
    for width in widths {
        painter.rect(
            Rect::from_min_size(Vec2::new(rect.min.x + 2.0, y), Vec2::new(width, 2.2)),
            Corners::capsule(),
            brand,
        );
        y += 3.4;
    }
    painter.text_in(
        Rect::from_min_size(
            Vec2::new(rect.min.x + 20.0, rect.min.y),
            Vec2::new(rect.width() - 20.0, rect.height()),
        ),
        "grana",
        TextStyle::new(font, palette.text_primary),
        Align::Center,
    );
}

fn open_menu(ui: &mut Ui, state: &State<'_>, commands: &mut Vec<Command>, rect: Rect) {
    let open = ui.popup_open(ui.id("open.menu"));
    let mut chosen = None;
    let busy = state.io.loading.is_some();
    grana_ui::popover::popover(ui, "open.menu", open, rect, 236.0, |ui| {
        if grana_ui::popover::action_row(ui, "open.image", Icon::Cube, "Open image…", None) {
            chosen = Some(Command::OpenImage);
        }
        if grana_ui::popover::action_row(
            ui,
            "open.folder",
            Icon::Folder,
            "Open OME-Zarr folder…",
            None,
        ) {
            chosen = Some(Command::OpenDataset);
        }
        grana_ui::popover::separator(ui);
        if grana_ui::popover::action_row(
            ui,
            "open.project",
            Icon::Folder,
            "Open project…",
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
            "Save project",
            Some(&format!("{COMMAND}S")),
        ) {
            chosen = Some(Command::SaveProject);
        }
        if grana_ui::popover::action_row(ui, "save.as", Icon::Save, "Save as…", None) {
            chosen = Some(Command::SaveProjectAs);
        }
        grana_ui::popover::separator(ui);
        if grana_ui::popover::action_row(ui, "snapshot", Icon::Camera, "Save snapshot…", None) {
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
    if !open {
        return;
    }
    let mut clear = false;
    grana_ui::popover::popover(ui, "header.diagnostics", open, anchor, 300.0, |ui| {
        crate::panel::section_header(ui, "Streaming", None);
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
            "Clear derived cache",
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
        format!(
            "level {} · {}/{}",
            view.level + 1,
            view.resident,
            view.demand
        )
    } else {
        format!(
            "level {} · {}/{} of {}",
            view.level + 1,
            view.resident,
            view.demand,
            view.visible
        )
    }
}
