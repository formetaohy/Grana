use grana_ui::{Align, Axis, Block, Color, Icon, Length, Rect, Role, TextStyle, Ui, Vec2};

use crate::panel::{card, card_title};
use crate::state::State;
use crate::widget;
use grana_session::Command;

pub fn build(ui: &mut Ui, state: &mut State<'_>, commands: &mut Vec<Command>) {
    let width = ui.available().x;
    let gap = 14.0;
    let left = ((width - gap) * 0.58).max(420.0);
    ui.row(Block::row().gaps(gap).align(Align::Start), |ui| {
        ui.column(
            Block::column().gaps(12.0).width(Length::Fixed(left)),
            |ui| {
                protocol(ui, state, commands);
                datasets(ui, state, commands);
            },
        );
        ui.column(Block::column().gaps(12.0).width(Length::Fill), |ui| {
            output(ui, state, commands);
        });
    });
}

fn protocol(ui: &mut Ui, state: &State<'_>, commands: &mut Vec<Command>) {
    let running = state.batch_running();
    card(ui, 8.0, |ui| {
        card_title(ui, Icon::Sliders, "Protocol", None);
        ui.row(Block::row().gaps(8.0).align(Align::Center), |ui| {
            if !running
                && widget::action_button(ui, "capture", Icon::Camera, "Capture", Role::Control)
                    .clicked
            {
                commands.push(Command::CaptureProtocol);
            }
            if !running
                && widget::action_button(ui, "load", Icon::Folder, "Load", Role::Quiet).clicked
            {
                commands.push(Command::LoadProtocol);
            }
            if !running
                && widget::action_button(ui, "save", Icon::Save, "Save", Role::Quiet).clicked
            {
                commands.push(Command::SaveProtocol);
            }
            if !running
                && state.batch_protocol().is_some()
                && widget::action_button(ui, "apply", Icon::Play, "Apply", Role::Quiet).clicked
            {
                commands.push(Command::ApplyProtocol);
            }
            if !running
                && let Some(text) = state.batch_protocol()
                && widget::action_button(ui, "copy", Icon::Copy, "Copy", Role::Quiet).clicked
            {
                ui.copy(text);
            }
            if state.batch_protocol().is_some() {
                ui.space(2.0);
                widget::badge(ui, "Ready", ui.theme().palette.success);
            }
        });
        match state.batch_protocol() {
            Some(text) => {
                let look = ui.theme().look(Role::Field, grana_ui::State::Rest);
                let style = TextStyle::new(
                    ui.theme().font(grana_ui::Level::Monospace),
                    ui.theme().palette.text_secondary,
                );
                ui.column(
                    Block::column()
                        .padding(grana_ui::Edges::symmetric(10.0, 8.0))
                        .gaps(4.0)
                        .background(look),
                    |ui| {
                        ui.text_style_wrapped(text, style, ui.available().x);
                    },
                );
            }
            None => {
                widget::weak(
                    ui,
                    "Capture the current region, objects and settings into a reusable protocol.",
                );
            }
        }
    });
}

fn datasets(ui: &mut Ui, state: &State<'_>, commands: &mut Vec<Command>) {
    let running = state.batch_running();
    card(ui, 8.0, |ui| {
        card_title(
            ui,
            Icon::Folder,
            "Datasets",
            Some(&state.batch_datasets().len().to_string()),
        );
        if state.batch_datasets().is_empty() {
            widget::weak(ui, "Choose a folder of datasets that share this protocol.");
        } else {
            let key = ui.id("batch.datasets");
            ui.scroll_axis(key, Axis::Column, Length::Fixed(136.0), None, |ui| {
                for (index, dataset) in state.batch_datasets().iter().enumerate() {
                    let row = ui.allocate_full(34.0);
                    let key = ui.id(&format!("dataset.{index}"));
                    let response = ui.interact(key, row);
                    let hover = ui.animate(
                        key,
                        if response.hovered { 1.0 } else { 0.0 },
                        grana_ui::Motion::quick(),
                    );
                    let tint = state_tint(dataset.state.as_str(), ui);
                    let icon_color = ui.theme().palette.text_tertiary;
                    let primary = ui.theme().palette.text_primary;
                    let callout = ui.theme().font(grana_ui::Level::Callout);
                    {
                        let mut painter = ui.painter();
                        if hover > 0.01 {
                            painter.rect(
                                row,
                                grana_ui::Corners::all(8.0),
                                Color::white_alpha(0.04 * hover),
                            );
                        }
                        grana_ui::icon::paint(
                            Icon::Folder,
                            &mut painter,
                            Vec2::new(row.min.x + 10.0, row.center().y),
                            14.0,
                            icon_color,
                            1.5,
                        );
                        painter.text_in(
                            Rect::from_min_max(
                                Vec2::new(row.min.x + 22.0, row.min.y),
                                Vec2::new(row.max.x - 100.0, row.max.y),
                            ),
                            &dataset.name,
                            TextStyle::new(callout, primary).truncating(),
                            Align::Center,
                        );
                    }
                    ui.place(
                        Rect::from_min_size(
                            Vec2::new(row.max.x - 92.0, row.center().y - 9.0),
                            Vec2::new(92.0, 18.0),
                        ),
                        |ui| {
                            widget::badge(ui, state_label(&dataset.state), tint);
                        },
                    );
                    if response.hovered && !dataset.detail.is_empty() {
                        ui.tooltip(&dataset.detail);
                    }
                }
            });
        }
        ui.row(Block::row().gaps(8.0).align(Align::Center), |ui| {
            if !running
                && widget::action_button(ui, "add", Icon::Plus, "Add folder", Role::Control).clicked
            {
                commands.push(Command::AddBatchDatasets);
            }
            if !running
                && !state.batch_datasets().is_empty()
                && widget::button(ui, "clear", "Clear", Role::Quiet)
            {
                commands.push(Command::ClearBatchDatasets);
            }
        });
    });
}

fn output(ui: &mut Ui, state: &State<'_>, commands: &mut Vec<Command>) {
    card(ui, 10.0, |ui| {
        card_title(ui, Icon::Export, "Output", None);
        match state.batch_output() {
            Some(path) => {
                let look = ui.theme().look(Role::Field, grana_ui::State::Rest);
                let style = TextStyle::new(
                    ui.theme().font(grana_ui::Level::Monospace),
                    ui.theme().palette.text_secondary,
                )
                .truncating();
                let rect = ui.allocate_full(30.0);
                {
                    let mut painter = ui.painter();
                    painter.outlined(rect, look.corners, look.fill, look.border, look.shadow);
                    painter.text_in(
                        rect.shrink2(grana_ui::Vec2::new(9.0, 0.0)),
                        path,
                        style,
                        Align::Center,
                    );
                }
            }
            None => widget::weak(ui, "Choose a folder for the exported tables."),
        }
        if !state.batch_running()
            && widget::action_button(ui, "output", Icon::Folder, "Choose folder", Role::Control)
                .clicked
        {
            commands.push(Command::ChooseBatchOutput);
        }
        if !state.batch_running() {
            let jobs = state.batch_jobs();
            ui.row(Block::row().gaps(8.0).align(Align::Center), |ui| {
                widget::field_label(ui, "Workers");
                if let Some(value) = widget::drag_value(ui, "batch.jobs", jobs as f32, 0.15, 0) {
                    let next = (value.round() as i64).clamp(1, 16) as u32;
                    if next as usize != jobs {
                        commands.push(Command::SetBatchJobs(next));
                    }
                }
                widget::caption(ui, &format!("{jobs} at a time"));
            });
            if let Some(value) = widget::toggle_row(
                ui,
                "batch.project",
                "Write a project per dataset",
                state.batch_project(),
            ) {
                commands.push(Command::SetBatchProject(value));
            }
        }
        ui.separator();
        let running = state.batch_running();
        if running {
            ui.row(Block::row().gaps(8.0).align(Align::Center), |ui| {
                widget::spinner(ui, 14.0);
                widget::caption(ui, "Running the protocol over every dataset");
                ui.space(1.0);
                if widget::button(ui, "cancel", "Cancel", Role::Quiet) {
                    commands.push(Command::CancelBatch);
                }
            });
        } else {
            let ready = state.batch_protocol().is_some()
                && !state.batch_datasets().is_empty()
                && state.batch_output().is_some();
            let role = if ready { Role::Prominent } else { Role::Quiet };
            if widget::button_with(ui, "run", "Run batch", role, Length::Fill).clicked && ready {
                commands.push(Command::RunBatch);
            }
        }
        if let Some(message) = state.batch_message() {
            let style = TextStyle::new(
                ui.theme().font(grana_ui::Level::Caption),
                ui.theme().palette.danger,
            )
            .wrapping();
            ui.text_style_wrapped(message, style, ui.available().x);
        }
        widget::weak(ui, "Re-running the same protocol resumes where it stopped.");
    });
}

fn state_tint(state: &str, ui: &Ui) -> Color {
    match state {
        "complete" | "done" | "resumed" => ui.theme().palette.success,
        "failed" => ui.theme().palette.danger,
        "running" => ui.theme().palette.warning,
        _ => ui.theme().palette.text_secondary,
    }
}

fn state_label(state: &str) -> &str {
    match state {
        "complete" | "done" => "Complete",
        "resumed" => "Resumed",
        "failed" => "Failed",
        "running" => "Running",
        "pending" => "Pending",
        "queued" => "Queued",
        other => other,
    }
}
