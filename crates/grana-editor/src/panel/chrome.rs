use glam::Vec2;
use grana_ui::{
    Align, Block, Border, Color, Corners, Edges, Fill, Icon, Length, Level, Motion, Rect, Role,
    Shadow, TextStyle, TypeStyle, Ui,
};

use crate::state::State;
use crate::widget;
use grana_session::Command;

use super::Panel;

const COMMAND: &str = if cfg!(target_os = "macos") {
    "⌘"
} else {
    "Ctrl "
};
const UNDO: &str = if cfg!(target_os = "macos") {
    "⌘Z"
} else {
    "Ctrl Z"
};
const REDO: &str = if cfg!(target_os = "macos") {
    "⇧⌘Z"
} else {
    "Ctrl Shift Z"
};

pub(super) fn toolbar_bar(
    ui: &mut Ui,
    rect: Rect,
    state: &mut State<'_>,
    commands: &mut Vec<Command>,
) {
    let look = ui.theme().look(Role::Toolbar, grana_ui::State::Rest);
    {
        let mut painter = ui.painter();
        painter.outlined(rect, look.corners, look.fill, look.border, look.shadow);
    }
    let height = rect.height();
    let left = Rect::from_min_size(rect.min, Vec2::new(430.0, height));
    let left_width = {
        let open = ui.measure_text(
            "Open",
            TextStyle::new(ui.theme().font(Level::Body), Color::WHITE),
        );
        let project = ui.measure_text(
            "Project",
            TextStyle::new(ui.theme().font(Level::Body), Color::WHITE),
        );
        16.0 + 26.0 + 1.0 + 6.0 + open.x + 34.0 + project.x + 34.0 + 32.0 + 16.0
    };
    let left = left.with_width(left_width);
    let right_width = {
        let analyze = ui.measure_text(
            "Analyze",
            TextStyle::new(ui.theme().font(Level::Callout), Color::WHITE),
        );
        let animate = ui.measure_text(
            "Animate",
            TextStyle::new(ui.theme().font(Level::Callout), Color::WHITE),
        );
        32.0 + 30.0 * 3.0 + 10.0 * 3.0 + (analyze.x + 22.0 + animate.x + 22.0 + 4.0)
    };
    let right = Rect::from_min_size(
        Vec2::new(rect.max.x - right_width, rect.min.y),
        Vec2::new(right_width, height),
    );
    let center_width = (right.min.x - left.max.x - 24.0).min(560.0);
    let center = Rect::from_center_size(
        Vec2::new(rect.center().x, rect.center().y),
        Vec2::new(center_width, height),
    );
    ui.place(left, |ui| {
        ui.row(
            Block::row()
                .padding(Edges::symmetric(16.0, 0.0))
                .gaps(8.0)
                .align(Align::Center)
                .height(Length::Fill),
            |ui| {
                brand(ui);
                vertical_rule(ui, 18.0);
                ui.space(6.0);
                open_menu(ui, state, commands);
                project_menu(ui, state, commands);
            },
        );
    });
    if center.width() > 160.0 {
        let title = TextStyle::new(
            TypeStyle::new(13.0, 600.0, 17.0),
            ui.theme().palette.text_primary,
        )
        .aligned(Align::Center)
        .truncating();
        let subtitle = TextStyle::new(
            TypeStyle::new(10.5, 400.0, 13.0),
            ui.theme().palette.text_tertiary,
        )
        .aligned(Align::Center)
        .truncating();
        let title_layout = ui.layout_text(state.scene.dataset.name(), title, center.width());
        let channels = state.scene.dataset.channels().len();
        let frames = state.scene.frame_count();
        let summary = format!(
            "{channels} channels · {frames} frames · {:.3} µm/px",
            state.scene.dataset.spacing().x()
        );
        let subtitle_layout = ui.layout_text(&summary, subtitle, center.width());
        let total = title_layout.size.y + subtitle_layout.size.y - 2.0;
        let origin = Vec2::new(center.min.x, center.center().y - total * 0.5);
        let title_color = ui.theme().palette.text_primary;
        let subtitle_color = ui.theme().palette.text_tertiary;
        let mut painter = ui.painter();
        painter.text(&title_layout, origin, title_color);
        painter.text(
            &subtitle_layout,
            Vec2::new(origin.x, origin.y + title_layout.size.y - 3.0),
            subtitle_color,
        );
    }
    ui.place(right, |ui| {
        ui.row(
            Block::row()
                .padding(Edges::symmetric(16.0, 0.0))
                .gaps(10.0)
                .align(Align::Center)
                .justify(Align::End)
                .width(Length::Fill)
                .height(Length::Fill),
            |ui| {
                let undo = state.history.undo;
                let redo = state.history.redo;
                let undo_tip = undo.map_or_else(
                    || "Nothing to undo".to_string(),
                    |action| format!("Undo {} · {UNDO}", action.label()),
                );
                let redo_tip = redo.map_or_else(
                    || "Nothing to redo".to_string(),
                    |action| format!("Redo {} · {REDO}", action.label()),
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
                mode_control(ui, state);
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
    let rect = ui.allocate(Vec2::new(1.0, height));
    let color = ui.theme().palette.separator;
    let mut painter = ui.painter();
    painter.rect(rect, Corners::ZERO, color);
}

fn mode_control(ui: &mut Ui, state: &mut State<'_>) {
    let labels = ["Analyze", "Animate"];
    let font = ui.theme().font(Level::Callout);
    let style = TextStyle::new(font, Color::WHITE);
    let widths = labels.map(|label| ui.measure_text(label, style).x + 22.0);
    let height = 30.0;
    let width = widths[0] + widths[1] + 4.0;
    let rect = ui.allocate(Vec2::new(width, height));
    let active = match state.workspace.panel {
        Some(Panel::Animation) => Some(1),
        Some(_) => Some(0),
        None => None,
    };
    let dark = ui.theme().dark;
    {
        let mut painter = ui.painter();
        painter.outlined(
            rect,
            Corners::capsule(),
            Fill::Solid(if dark {
                Color::rgba(0.0, 0.0, 0.0, 0.24)
            } else {
                Color::rgba(1.0, 1.0, 1.0, 0.55)
            }),
            Border {
                width: 0.5,
                color: if dark {
                    Color::white_alpha(0.06)
                } else {
                    Color::black_alpha(0.05)
                },
            },
            Shadow::NONE,
        );
    }
    let mut chosen = None;
    let mut left = rect.min.x + 2.0;
    for (index, label) in labels.iter().enumerate() {
        let cell = Rect::from_min_size(
            Vec2::new(left, rect.min.y + 2.0),
            Vec2::new(widths[index], height - 4.0),
        );
        let key = ui.id(&format!("mode.{index}"));
        let response = ui.interact(key, cell);
        if response.clicked {
            chosen = Some(index);
        }
        let hover = ui.animate(
            key,
            if response.hovered { 1.0 } else { 0.0 },
            Motion::quick(),
        );
        let selected = active == Some(index);
        let role = if selected {
            Role::SegmentSelected
        } else {
            Role::Segment
        };
        let look = ui.look(key, role, &response);
        let color = if selected {
            ui.theme().palette.text_primary
        } else {
            ui.theme()
                .palette
                .text_secondary
                .lerp(ui.theme().palette.text_primary, hover)
        };
        let mut painter = ui.painter();
        painter.outlined(
            cell,
            Corners::capsule(),
            look.fill,
            look.border,
            look.shadow,
        );
        painter.text_in(
            cell,
            label,
            TextStyle::new(font, color).aligned(Align::Center),
            Align::Center,
        );
        left += widths[index];
    }
    if let Some(index) = chosen {
        let panel = if index == 0 {
            if active == Some(0) {
                None
            } else {
                Some(Panel::Statistics)
            }
        } else if active == Some(1) {
            None
        } else {
            Some(Panel::Animation)
        };
        state.workspace.panel = panel;
    }
}

fn menu_button(ui: &mut Ui, id: &str, label: &str) -> Rect {
    let key = ui.id(id);
    let font = ui.theme().font(Level::Body);
    let width = ui.measure_text(label, TextStyle::new(font, Color::WHITE)).x + 34.0;
    let height = 28.0;
    let rect = ui.allocate(Vec2::new(width, height));
    let response = ui.interact(key, rect);
    if response.clicked {
        ui.toggle_popup(key);
    }
    let look = ui.look(key, Role::Quiet, &response);
    let chevron = ui.theme().palette.text_tertiary;
    {
        let mut painter = ui.painter();
        painter.outlined(rect, look.corners, look.fill, look.border, look.shadow);
        painter.text_in(
            Rect::from_min_max(
                rect.min + Vec2::new(9.0, 0.0),
                rect.max - Vec2::new(18.0, 0.0),
            ),
            label,
            TextStyle::new(font, look.text),
            Align::Center,
        );
        let centre = Vec2::new(rect.max.x - 12.0, rect.center().y);
        painter.segment(
            centre + Vec2::new(-3.2, -1.7),
            centre + Vec2::new(0.0, 1.5),
            1.5,
            chevron,
        );
        painter.segment(
            centre + Vec2::new(0.0, 1.5),
            centre + Vec2::new(3.2, -1.7),
            1.5,
            chevron,
        );
    }
    rect
}

fn open_menu(ui: &mut Ui, state: &State<'_>, commands: &mut Vec<Command>) {
    let rect = menu_button(ui, "open.menu", "Open");
    let open = ui.popup_open(ui.id("open.menu"));
    let mut chosen = None;
    let busy = state.io.loading.is_some();
    grana_ui::popover::popover(ui, "open.menu", open, rect, 240.0, |ui| {
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
    });
    if let Some(action) = chosen {
        ui.close_popup();
        if !busy {
            commands.push(action);
        }
    }
}

fn project_menu(ui: &mut Ui, state: &State<'_>, commands: &mut Vec<Command>) {
    let rect = menu_button(ui, "project.menu", "Project");
    let key = ui.id("project.menu");
    let open = ui.popup_open(key);
    let mut chosen = None;
    let busy = state.io.loading.is_some();
    let open_key = format!("{COMMAND}O");
    let save_key = format!("{COMMAND}S");
    grana_ui::popover::popover(ui, "project.menu", open, rect, 224.0, |ui| {
        if grana_ui::popover::action_row(
            ui,
            "open.project",
            Icon::Folder,
            "Open project",
            Some(&open_key),
        ) {
            chosen = Some(Command::OpenProject);
        }
        grana_ui::popover::separator(ui);
        if grana_ui::popover::action_row(
            ui,
            "save.project",
            Icon::Save,
            "Save project",
            Some(&save_key),
        ) {
            chosen = Some(Command::SaveProject);
        }
        if grana_ui::popover::action_row(ui, "save.as", Icon::Save, "Save as…", None) {
            chosen = Some(Command::SaveProjectAs);
        }
        grana_ui::popover::separator(ui);
        if grana_ui::popover::action_row(ui, "snapshot", Icon::Camera, "Save snapshot", None) {
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
