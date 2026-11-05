use glam::Vec2;
use grana_ui::glass::GlassStyle;
use grana_ui::{
    Align, Block, Color, Corners, Icon, Length, Level, Motion, Rect, Role, TextStyle, Ui, icon,
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
    ui.glass(ui.id("toolbar.glass"), rect, GlassStyle::bar());
    let inner = rect.shrink2(Vec2::new(14.0, 0.0));
    let leading = Rect::from_min_size(
        inner.min,
        Vec2::new((inner.width() * 0.42).max(260.0), inner.height()),
    );
    ui.place(leading, |ui| {
        ui.row(
            Block::row()
                .gaps(6.0)
                .align(Align::Center)
                .height(Length::Fill),
            |ui| {
                let hidden = state.workspace.sidebar_hidden;
                if widget::icon_button(
                    ui,
                    "sidebar.toggle",
                    Icon::Sidebar,
                    32.0,
                    Role::Quiet,
                    if hidden {
                        "Show sidebar"
                    } else {
                        "Hide sidebar"
                    },
                ) {
                    state.workspace.sidebar_hidden = !hidden;
                }
                brand(ui);
                open_menu(ui, state, commands);
                project_menu(ui, state, commands);
            },
        );
    });
    let trailing = Rect::from_min_max(
        Vec2::new((inner.max.x - 470.0).max(inner.min.x), inner.min.y),
        inner.max,
    );
    let free = Rect::from_min_max(
        Vec2::new(leading.max.x + 24.0, inner.min.y),
        Vec2::new(trailing.min.x - 24.0, inner.max.y),
    );
    if free.width() > 140.0 {
        let style = TextStyle::new(
            ui.theme().font(Level::Body).medium(),
            ui.theme().palette.text_secondary,
        )
        .aligned(Align::Center)
        .truncating();
        ui.place(free, |ui| {
            ui.text_in(free, state.scene.dataset.name(), style, Align::Center);
        });
    }
    ui.place(trailing, |ui| {
        ui.row(
            Block::row()
                .gaps(2.0)
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
                if widget::icon_button_enabled(
                    ui,
                    "undo",
                    Icon::Undo,
                    30.0,
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
                    30.0,
                    Role::Quiet,
                    &redo_tip,
                    redo.is_some(),
                ) {
                    commands.push(Command::Redo);
                }
                vertical_rule(ui, 20.0);
                if widget::icon_button(
                    ui,
                    "fit",
                    Icon::Fit,
                    30.0,
                    Role::Quiet,
                    "Fit volume to view",
                ) {
                    crate::view::fit_volume(state);
                }
                vertical_rule(ui, 20.0);
                let open = state.workspace.panel.is_some();
                let role = if open { Role::Tinted } else { Role::Quiet };
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
                    30.0,
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
    let rect = ui.allocate(Vec2::new(34.0, 30.0));
    let brand = ui.theme().palette.brand;
    let mut painter = ui.painter();
    let widths = [7.0, 8.2, 9.4, 10.6, 11.8];
    let mut y = rect.center().y - 7.75;
    for width in widths {
        painter.rect(
            Rect::from_min_size(Vec2::new(rect.min.x + 4.0, y), Vec2::new(width, 2.1)),
            Corners::capsule(),
            brand,
        );
        y += 3.35;
    }
    let style = TextStyle::new(
        ui.theme().font(Level::Title),
        ui.theme().palette.text_primary,
    );
    ui.text_in(
        Rect::from_min_size(
            Vec2::new(rect.max.x + 2.0, rect.min.y),
            Vec2::new(64.0, rect.height()),
        ),
        "grana",
        style,
        Align::Center,
    );
    ui.space(64.0);
}

fn vertical_rule(ui: &mut Ui, height: f32) {
    let rect = ui.allocate(Vec2::new(11.0, height));
    let palette = ui.theme().palette;
    let mut painter = ui.painter();
    painter.rect(
        Rect::from_center_size(rect.center(), Vec2::new(1.0, height - 6.0)),
        Corners::ZERO,
        palette.separator,
    );
}

fn menu_button(ui: &mut Ui, id: &str, label: &str, dot: bool, maximum: f32) -> Rect {
    let key = ui.id(id);
    let palette = ui.theme().palette;
    let font = ui.theme().font(Level::Callout).medium();
    let measured = ui.measure_text(label, TextStyle::new(font, Color::WHITE)).x;
    let label_width = measured.min(maximum);
    let width = label_width + if dot { 30.0 } else { 26.0 };
    let height = 30.0;
    let rect = ui.allocate(Vec2::new(width, height));
    let mut response = ui.interact(key, rect);
    let corners = Corners::capsule();
    if ui.focusable(key, rect, corners) && ui.activated(key) {
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
    {
        let mut painter = ui.painter();
        if hover > 0.01 {
            painter.rect(
                rect,
                corners,
                palette.material_hover.with_alpha(0.85 * hover),
            );
        }
        painter.text_in(
            Rect::from_min_max(
                Vec2::new(rect.min.x + 11.0, rect.min.y),
                Vec2::new(rect.min.x + 11.0 + label_width, rect.max.y),
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
                    Vec2::new(rect.min.x + 16.0 + label_width, rect.center().y),
                    Vec2::splat(5.0),
                ),
                Corners::capsule(),
                palette.warning,
            );
        }
        icon::paint(
            Icon::ChevronDown,
            &mut painter,
            Vec2::new(rect.max.x - 10.0, rect.center().y),
            9.0,
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

fn project_menu(ui: &mut Ui, state: &State<'_>, commands: &mut Vec<Command>) {
    let name = state.io.project.as_deref().unwrap_or("Untitled");
    let rect = menu_button(ui, "project.menu", name, state.io.dirty, 168.0);
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
    grana_ui::popover::popover(ui, "header.diagnostics", open, anchor, 296.0, |ui| {
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
