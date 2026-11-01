use glam::Vec2;
use grana_ui::{
    Align, Block, Border, Color, Corners, Edges, Fill, Icon, Length, Level, Motion, Rect, Role,
    Shadow, TextStyle, TypeStyle, Ui,
};

use crate::action::Action;
use crate::state::State;
use crate::widget;

use super::Panel;

pub(super) fn toolbar_bar(ui: &mut Ui, rect: Rect, state: &State<'_>, actions: &mut Vec<Action>) {
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
        16.0 + 26.0 + 1.0 + 6.0 + open.x + 42.0 + project.x + 34.0 + 32.0 + 16.0
    };
    let left = left.with_width(left_width);
    let right_width = {
        let fit = 32.0;
        let analyze = ui.measure_text(
            "Analyze",
            TextStyle::new(ui.theme().font(Level::Callout), Color::WHITE),
        );
        let animate = ui.measure_text(
            "Animate",
            TextStyle::new(ui.theme().font(Level::Callout), Color::WHITE),
        );
        fit + 12.0 + (analyze.x + 22.0 + animate.x + 22.0 + 4.0) + 14.0
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
                if widget::action_button(ui, "open", Icon::Folder, "Open", Role::Quiet).clicked {
                    actions.push(Action::OpenDataset);
                }
                project_menu(ui, state, actions);
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
                .padding(Edges::symmetric(14.0, 0.0))
                .gaps(10.0)
                .align(Align::Center)
                .justify(Align::End)
                .height(Length::Fill),
            |ui| {
                if widget::icon_button(
                    ui,
                    "fit",
                    Icon::Fit,
                    30.0,
                    Role::Quiet,
                    "Fit volume to view",
                ) {
                    actions.push(Action::Fit);
                }
                mode_control(ui, state, actions);
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

fn mode_control(ui: &mut Ui, state: &State<'_>, actions: &mut Vec<Action>) {
    let labels = ["Analyze", "Animate"];
    let font = ui.theme().font(Level::Callout);
    let style = TextStyle::new(font, Color::WHITE);
    let widths = labels.map(|label| ui.measure_text(label, style).x + 22.0);
    let height = 30.0;
    let width = widths[0] + widths[1] + 4.0;
    let rect = ui.allocate(Vec2::new(width, height));
    let active = match *state.panel {
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
        actions.push(Action::SetPanel(panel));
    }
}

fn project_menu(ui: &mut Ui, state: &State<'_>, actions: &mut Vec<Action>) {
    let key = ui.id("project.menu");
    let font = ui.theme().font(Level::Body);
    let width = ui
        .measure_text("Project", TextStyle::new(font, Color::WHITE))
        .x
        + 34.0;
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
            "Project",
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
    let open = ui.popup_open(key);
    let mut chosen = None;
    let busy = state.loading.is_some();
    grana_ui::popover::popover(ui, "project.menu", open, rect, 224.0, |ui| {
        if grana_ui::popover::action_row(
            ui,
            "open.project",
            Icon::Folder,
            "Open project",
            Some("Ctrl O"),
        ) {
            chosen = Some(Action::OpenProject);
        }
        grana_ui::popover::separator(ui);
        if grana_ui::popover::action_row(
            ui,
            "save.project",
            Icon::Save,
            "Save project",
            Some("Ctrl S"),
        ) {
            chosen = Some(Action::SaveProject);
        }
        if grana_ui::popover::action_row(ui, "save.as", Icon::Save, "Save as…", None) {
            chosen = Some(Action::SaveProjectAs);
        }
        grana_ui::popover::separator(ui);
        if grana_ui::popover::action_row(ui, "snapshot", Icon::Camera, "Save snapshot", None) {
            chosen = Some(Action::SaveSnapshot);
        }
    });
    if let Some(action) = chosen {
        ui.close_popup();
        if !busy {
            actions.push(action);
        }
    }
}
