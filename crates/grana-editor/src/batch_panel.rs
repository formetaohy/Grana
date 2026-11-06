use grana_ui::{Align, Axis, Block, Color, Icon, Length, Rect, Role, TextStyle, Ui, Vec2};

use crate::panel::{card, card_title, divider, form_row};
use crate::state::State;
use crate::widget;
use grana_session::Command;

pub fn build(ui: &mut Ui, state: &mut State<'_>, commands: &mut Vec<Command>) {
    let width = ui.available().x;
    let gap = 12.0;
    let col_a = ((width - gap * 2.0) * 0.40).max(360.0);
    let col_b = ((width - gap * 2.0) * 0.32).max(280.0);
    ui.row(Block::row().gaps(gap).align(Align::Start), |ui| {
        ui.column(
            Block::column().gaps(8.0).width(Length::Fixed(col_a)),
            |ui| {
                protocol(ui, state, commands);
                datasets(ui, state, commands);
            },
        );
        ui.column(
            Block::column().gaps(8.0).width(Length::Fixed(col_b)),
            |ui| {
                output(ui, state, commands);
            },
        );
        ui.column(Block::column().gaps(8.0).width(Length::Fill), |ui| {
            comparison(ui, state);
        });
    });
}

fn protocol(ui: &mut Ui, state: &State<'_>, commands: &mut Vec<Command>) {
    let running = state.batch_running();
    let has_protocol = state.batch_protocol().is_some();
    card(ui, 6.0, |ui| {
        card_title(
            ui,
            Icon::Sliders,
            "Protocol",
            if has_protocol { Some("Ready") } else { None },
        );
        ui.row(Block::row().gaps(6.0).align(Align::Center), |ui| {
            if !running
                && widget::action_button(ui, "capture", Icon::Camera, "Capture", Role::Control)
                    .clicked
            {
                commands.push(Command::CaptureProtocol);
            }
            if !running && widget::button(ui, "load", "Load", Role::Quiet) {
                commands.push(Command::LoadProtocol);
            }
            if !running && widget::button(ui, "save", "Save", Role::Quiet) {
                commands.push(Command::SaveProtocol);
            }
            if !running && has_protocol && widget::button(ui, "apply", "Apply", Role::Quiet) {
                commands.push(Command::ApplyProtocol);
            }
            if !running
                && let Some(text) = state.batch_protocol()
                && widget::button(ui, "copy", "Copy", Role::Quiet)
            {
                ui.copy(text);
            }
        });
        match state.batch_protocol() {
            Some(text) => {
                let look = ui.theme().surface(Role::Field, grana_ui::State::Rest);
                let style = TextStyle::new(
                    ui.theme().font(grana_ui::Level::Monospace),
                    ui.theme().palette.text_secondary,
                )
                .wrapping();
                ui.column(
                    Block::column()
                        .padding(grana_ui::Edges::symmetric(10.0, 6.0))
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
    let count = state.batch_datasets().len();
    card(ui, 6.0, |ui| {
        card_title(
            ui,
            Icon::Folder,
            "Datasets",
            Some(&format!(
                "{count} item{}",
                if count == 1 { "" } else { "s" }
            )),
        );
        if state.batch_datasets().is_empty() {
            widget::weak(ui, "Choose a folder of datasets that share this protocol.");
        } else {
            let key = ui.id("batch.datasets");
            ui.scroll_axis(key, Axis::Column, Length::Fixed(96.0), None, |ui| {
                for (index, dataset) in state.batch_datasets().iter().enumerate() {
                    let row = ui.allocate_full(30.0);
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
                && widget::action_button(ui, "add", Icon::Plus, "Add Folder…", Role::Control)
                    .clicked
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
    card(ui, 7.0, |ui| {
        card_title(ui, Icon::Export, "Output & Execution", None);
        if let Some(path) = state.batch_output() {
            let look = ui.theme().surface(Role::Field, grana_ui::State::Rest);
            let style = TextStyle::new(
                ui.theme().font(grana_ui::Level::Monospace),
                ui.theme().palette.text_secondary,
            )
            .truncating();
            let rect = ui.allocate_full(28.0);
            let mut painter = ui.painter();
            painter.outlined(rect, look.corners, look.fill, look.border, look.shadow);
            painter.text_in(
                rect.shrink2(Vec2::new(10.0, 0.0)),
                path,
                style,
                Align::Center,
            );
        } else {
            widget::weak(ui, "Choose a destination folder for the exported tables.");
        }
        if !state.batch_running() {
            if widget::action_button(ui, "output", Icon::Folder, "Choose Folder…", Role::Control)
                .clicked
            {
                commands.push(Command::ChooseBatchOutput);
            }
            let jobs = state.batch_jobs();
            form_row(ui, "Workers", |ui| {
                ui.row(Block::row().gaps(8.0).align(Align::Center), |ui| {
                    if let Some(value) = widget::drag_value(ui, "batch.jobs", jobs as f32, 0.15, 0)
                    {
                        let next = (value.round() as i64).clamp(1, 16) as u32;
                        if next as usize != jobs {
                            commands.push(Command::SetBatchJobs(next));
                        }
                    }
                    widget::caption(ui, &format!("{jobs} concurrent"));
                });
            });
            if let Some(value) = widget::toggle_row(
                ui,
                "batch.project",
                "Save Project per Dataset",
                state.batch_project(),
            ) {
                commands.push(Command::SetBatchProject(value));
            }
        }
        divider(ui);
        let running = state.batch_running();
        if running {
            let paused = state.batch_paused();
            ui.row(Block::row().gaps(8.0).align(Align::Center), |ui| {
                widget::spinner(ui, 14.0);
                widget::caption(
                    ui,
                    if paused {
                        "Paused between analysis steps"
                    } else {
                        "Running protocol across datasets…"
                    },
                );
                ui.space(1.0);
                if paused {
                    if widget::action_button(ui, "resume", Icon::Play, "Resume", Role::Quiet)
                        .clicked
                    {
                        commands.push(Command::ResumeBatch);
                    }
                } else if widget::action_button(ui, "pause", Icon::Pause, "Pause", Role::Quiet)
                    .clicked
                {
                    commands.push(Command::PauseBatch);
                }
                if widget::button(ui, "cancel", "Cancel", Role::Quiet) {
                    commands.push(Command::CancelBatch);
                }
            });
        } else {
            let ready = state.batch_protocol().is_some()
                && !state.batch_datasets().is_empty()
                && state.batch_output().is_some();
            if widget::button_with_enabled(
                ui,
                "run",
                "Run Batch",
                Role::Prominent,
                Length::Fill,
                ready,
            )
            .clicked
            {
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

fn comparison(ui: &mut Ui, state: &State<'_>) {
    let rows = state.batch_comparison();
    card(ui, 6.0, |ui| {
        card_title(
            ui,
            Icon::Table,
            "Comparison",
            (!rows.is_empty())
                .then(|| format!("{} groups", rows.len()))
                .as_deref(),
        );
        if rows.is_empty() {
            widget::weak(
                ui,
                "Run the batch to compare reported groups in comparison.csv.",
            );
            return;
        }
        let key = ui.id("batch.comparison");
        ui.scroll_axis(key, Axis::Column, Length::Fixed(180.0), None, |ui| {
            for (index, row) in rows.iter().enumerate() {
                entry(ui, index, rows.len(), row);
            }
        });
    });
}

fn entry(ui: &mut Ui, index: usize, rows: usize, row: &grana_session::BatchComparison) {
    let palette = ui.theme().palette;
    let callout = ui.theme().font(grana_ui::Level::Callout);
    let caption = ui.theme().font(grana_ui::Level::Caption);
    ui.column(Block::column().gaps(2.0), |ui| {
        let head = ui.allocate_full(18.0);
        let mut painter = ui.painter();
        painter.text_in(
            head,
            &format!("{} · {}", row.group, row.name),
            TextStyle::new(callout, palette.text_primary).truncating(),
            Align::Center,
        );
        painter.text_in(
            head,
            row.kind.as_str(),
            TextStyle::new(caption, palette.text_tertiary).aligned(Align::End),
            Align::Center,
        );
        let mut parts = vec![format!(
            "{} dataset{}",
            row.datasets,
            if row.datasets == 1 { "" } else { "s" }
        )];
        if let Some(objects) = row.objects {
            parts.push(format!("{objects:.1} objects"));
        }
        if let Some(volume) = row.volume {
            parts.push(format!(
                "Volume {} µm³",
                spread(volume, row.volume_deviation)
            ));
        }
        if let Some(area) = row.area {
            parts.push(format!("Area {} µm²", spread(area, row.area_deviation)));
        }
        if let Some(size) = row.size {
            parts.push(format!("Size {} µm", spread(size, row.size_deviation)));
        }
        if let Some(intensity) = row.intensity {
            parts.push(format!(
                "Intensity {}",
                spread(intensity, row.intensity_deviation)
            ));
        }
        let detail = TextStyle::new(caption, palette.text_secondary).wrapping();
        ui.text_style_wrapped(&parts.join(" · "), detail, ui.available().x);
    });
    if index + 1 < rows {
        divider(ui);
    }
}

fn spread(mean: f64, deviation: Option<f64>) -> String {
    match deviation {
        Some(deviation) => format!("{mean:.3} ±{deviation:.3}"),
        None => format!("{mean:.3}"),
    }
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
