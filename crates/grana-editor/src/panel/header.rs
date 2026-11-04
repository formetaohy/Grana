use glam::Vec2;
use grana_ui::{
    Align, Block, Color, Corners, Edges, Icon, Length, Level, Motion, Rect, Role, TextStyle, Ui,
};

use crate::state::{State, ViewStats};
use crate::widget;
use grana_session::Command;

const COMMAND: &str = if cfg!(target_os = "macos") {
    "⌘"
} else {
    "Ctrl "
};

pub(super) fn bar(ui: &mut Ui, rect: Rect, state: &mut State<'_>, commands: &mut Vec<Command>) {
    let left = Rect::from_min_max(rect.min, Vec2::new(rect.max.x * 0.5, rect.max.y));
    ui.place(left, |ui| {
        ui.row(
            Block::row()
                .padding(Edges::symmetric(14.0, 0.0))
                .gaps(6.0)
                .align(Align::Center)
                .height(Length::Fill),
            |ui| {
                brand(ui);
                ui.space(2.0);
                vertical_rule(ui, 18.0);
                ui.space(2.0);
                open_menu(ui, state, commands);
                project_menu(ui, state, commands);
            },
        );
    });
    let right = Rect::from_min_max(Vec2::new(rect.max.x * 0.5, rect.min.y), rect.max);
    ui.place(right, |ui| {
        ui.row(
            Block::row()
                .padding(Edges::symmetric(14.0, 0.0))
                .gaps(4.0)
                .align(Align::Center)
                .width(Length::Fill)
                .height(Length::Fill),
            |ui| {
                let analysis_width =
                    ui.measure_text(
                        "Analysis",
                        TextStyle::new(ui.theme().button_font(), Color::WHITE),
                    )
                    .x + 44.0;
                let content = 28.0 * 3.0 + 9.0 * 2.0 + analysis_width + 6.0 * 4.0;
                ui.space((ui.available().x - content).max(0.0));
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
                if widget::icon_button_enabled(
                    ui,
                    "undo",
                    Icon::Undo,
                    28.0,
                    Role::Quiet,
                    &undo_tip,
                    undo.is_some(),
                ) {
                    commands.push(Command::Undo);
                }
                if widget::icon_button_enabled(
                    ui,
                    "redo",
                    Icon::Redo,
                    28.0,
                    Role::Quiet,
                    &redo_tip,
                    redo.is_some(),
                ) {
                    commands.push(Command::Redo);
                }
                vertical_rule(ui, 18.0);
                if widget::icon_button(
                    ui,
                    "fit",
                    Icon::Fit,
                    28.0,
                    Role::Quiet,
                    "Fit volume to view",
                ) {
                    crate::view::fit_volume(state);
                }
                vertical_rule(ui, 18.0);
                let open = state.workspace.panel.is_some();
                let role = if open { Role::Tinted } else { Role::Control };
                if widget::action_button(ui, "analysis", Icon::Table, "Analysis", role).clicked {
                    state.workspace.panel = if open {
                        None
                    } else {
                        let last = ui
                            .value_of(ui.id("dock.last"))
                            .map_or(0, |value| value.round().clamp(0.0, 4.0) as usize);
                        Some(crate::panel::Panel::ALL[last])
                    };
                }
                let key = ui.id("header.diagnostics");
                let response = widget::icon_button_response(
                    ui,
                    "header.diagnostics",
                    Icon::Info,
                    28.0,
                    Role::Quiet,
                    "Streaming diagnostics",
                );
                if response.clicked {
                    ui.toggle_popup(key);
                }
                diagnostics(ui, state, commands, response.rect);
            },
        );
    });
}

fn brand(ui: &mut Ui) {
    let rect = ui.allocate(Vec2::splat(26.0));
    let brand = ui.theme().palette.brand;
    let widths = [7.0, 8.2, 9.4, 10.6, 11.8];
    let mut y = rect.center().y - 7.75;
    let mut painter = ui.painter();
    for width in widths {
        painter.rect(
            Rect::from_min_size(Vec2::new(rect.center().x - 5.9, y), Vec2::new(width, 2.1)),
            Corners::capsule(),
            brand,
        );
        y += 3.35;
    }
}

fn vertical_rule(ui: &mut Ui, height: f32) {
    let rect = ui.allocate(Vec2::new(9.0, height));
    let palette = ui.theme().palette;
    let mut painter = ui.painter();
    painter.rect(
        Rect::from_center_size(rect.center(), Vec2::new(1.0, height - 4.0)),
        Corners::ZERO,
        palette.border,
    );
}

fn menu_button(ui: &mut Ui, id: &str, label: &str, dot: bool, maximum: f32) -> Rect {
    let key = ui.id(id);
    let palette = ui.theme().palette;
    let font = ui.theme().font(Level::Callout).medium();
    let measured = ui.measure_text(label, TextStyle::new(font, Color::WHITE)).x;
    let label_width = measured.min(maximum);
    let width = label_width + if dot { 26.0 } else { 22.0 };
    let rect = ui.allocate(Vec2::new(width, 26.0));
    let mut response = ui.interact(key, rect);
    let corners = Corners::all(ui.theme().metrics.radius - 2.0);
    if ui.focusable(key, rect, corners) && ui.activated(key) {
        response.clicked = true;
    }
    if response.clicked {
        ui.toggle_popup(key);
    }
    let hover = ui.animate(
        key,
        if response.hovered || ui.popup_open(key) {
            1.0
        } else {
            0.0
        },
        Motion::quick(),
    );
    let radius = ui.theme().metrics.radius - 2.0;
    {
        let mut painter = ui.painter();
        if hover > 0.01 {
            painter.rect(rect, Corners::all(radius), palette.raised);
        }
        painter.text_in(
            Rect::from_min_max(
                Vec2::new(rect.min.x + 8.0, rect.min.y),
                Vec2::new(rect.min.x + 8.0 + label_width, rect.max.y),
            ),
            label,
            TextStyle::new(
                font,
                palette.text_secondary.lerp(palette.text_primary, hover),
            )
            .truncating(),
            Align::Center,
        );
        if dot {
            painter.rect(
                Rect::from_center_size(
                    Vec2::new(rect.min.x + 13.0 + label_width, rect.center().y),
                    Vec2::splat(5.0),
                ),
                Corners::capsule(),
                palette.warning,
            );
        }
        grana_ui::icon::paint(
            Icon::ChevronDown,
            &mut painter,
            Vec2::new(rect.max.x - 11.0, rect.center().y),
            10.0,
            palette.text_tertiary,
            1.5,
        );
    }
    rect
}

fn open_menu(ui: &mut Ui, state: &State<'_>, commands: &mut Vec<Command>) {
    let rect = menu_button(ui, "open.menu", "Open", false, 80.0);
    let open = ui.popup_open(ui.id("open.menu"));
    let mut chosen = None;
    let busy = state.io.loading.is_some();
    grana_ui::popover::popover(ui, "open.menu", open, rect, 232.0, |ui| {
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

fn project_menu(ui: &mut Ui, state: &State<'_>, commands: &mut Vec<Command>) {
    let name = state.io.project.as_deref().unwrap_or("Untitled");
    let rect = menu_button(ui, "project.menu", name, state.io.dirty, 150.0);
    let open = ui.popup_open(ui.id("project.menu"));
    let mut chosen = None;
    let busy = state.io.loading.is_some();
    grana_ui::popover::popover(ui, "project.menu", open, rect, 224.0, |ui| {
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
    grana_ui::popover::popover(ui, "header.diagnostics", open, anchor, 288.0, |ui| {
        crate::panel::section_header(ui, "Streaming", None);
        widget::labelled(ui, "volume", &bricks(state.frame.volume));
        widget::labelled(ui, "section", &bricks(state.frame.section));
        widget::labelled(
            ui,
            "levels",
            &format!(
                "{} levels · {} stored",
                state.scene.dataset.level_count(),
                state.scene.dataset.stored_levels()
            ),
        );
        widget::labelled(
            ui,
            "bricks",
            &format!("{:.0} MiB", state.frame.bytes as f64 / (1024.0 * 1024.0)),
        );
        widget::labelled(
            ui,
            "geometry",
            &format!(
                "{:.0} MiB · {:.0} MiB disk",
                state.scene.geometry_bytes() as f64 / (1024.0 * 1024.0),
                state.io.store.spilled as f64 / (1024.0 * 1024.0)
            ),
        );
        widget::labelled(
            ui,
            "transparent",
            &format!(
                "{:.0} MiB",
                state.frame.fragments as f64 / (1024.0 * 1024.0)
            ),
        );
        if state.io.pending > 0 {
            ui.row(Block::row().gaps(6.0).align(Align::Center), |ui| {
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
