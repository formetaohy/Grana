use egui::{RichText, Ui};

use crate::action::Action;
use crate::state::State;

pub fn build(ui: &mut Ui, state: &mut State<'_>, actions: &mut Vec<Action>) {
    header(ui, state, actions);
    ui.separator();
    datasets(ui, state, actions);
    ui.separator();
    footer(ui, state, actions);
}

fn header(ui: &mut Ui, state: &State<'_>, actions: &mut Vec<Action>) {
    let running = state.batch_running;
    ui.horizontal(|ui| {
        ui.strong("Protocol");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add_enabled(!running, egui::Button::new("Save"))
                .clicked()
            {
                actions.push(Action::SaveProtocol);
            }
            if ui
                .add_enabled(!running, egui::Button::new("Load"))
                .clicked()
            {
                actions.push(Action::LoadProtocol);
            }
            if ui
                .add_enabled(!running, egui::Button::new("Capture"))
                .clicked()
            {
                actions.push(Action::CaptureProtocol);
            }
        });
    });
    match state.batch_protocol {
        Some(protocol) => {
            ui.label(RichText::new(protocol).small());
        }
        None => {
            ui.label(
                RichText::new("capture the current objects, parameters and colocalization pair")
                    .small()
                    .weak(),
            );
        }
    }
}

fn datasets(ui: &mut Ui, state: &State<'_>, actions: &mut Vec<Action>) {
    let running = state.batch_running;
    ui.horizontal(|ui| {
        ui.strong("Datasets");
        ui.label(
            RichText::new(format!("{}", state.batch_datasets.len()))
                .small()
                .weak(),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add_enabled(!running, egui::Button::new("Add folder"))
                .clicked()
            {
                actions.push(Action::AddBatchDatasets);
            }
            if ui
                .add_enabled(
                    !running && !state.batch_datasets.is_empty(),
                    egui::Button::new("Clear"),
                )
                .clicked()
            {
                actions.push(Action::ClearBatchDatasets);
            }
        });
    });
    if state.batch_datasets.is_empty() {
        ui.label(
            RichText::new("a folder of OME-Zarr datasets that share this protocol")
                .small()
                .weak(),
        );
        return;
    }
    egui::ScrollArea::vertical()
        .id_salt("grana.batch.datasets")
        .max_height(116.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            for dataset in state.batch_datasets {
                ui.horizontal(|ui| {
                    ui.monospace(dataset.name.as_str());
                    ui.label(RichText::new(dataset.state.as_str()).small().weak());
                    if !dataset.detail.is_empty() {
                        ui.label(RichText::new(dataset.detail.as_str()).small().weak());
                    }
                });
            }
        });
}

fn footer(ui: &mut Ui, state: &State<'_>, actions: &mut Vec<Action>) {
    ui.horizontal(|ui| {
        match state.batch_output {
            Some(output) => {
                ui.label(RichText::new(output).small().weak());
            }
            None => {
                ui.label(
                    RichText::new("choose a folder for the statistics")
                        .small()
                        .weak(),
                );
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if state.batch_running {
                if ui.button("Cancel").clicked() {
                    actions.push(Action::CancelBatch);
                }
                ui.spinner();
            } else {
                let ready = state.batch_protocol.is_some()
                    && !state.batch_datasets.is_empty()
                    && state.batch_output.is_some();
                if ui.add_enabled(ready, egui::Button::new("Run")).clicked() {
                    actions.push(Action::RunBatch);
                }
                if ui.button("Output").clicked() {
                    actions.push(Action::ChooseBatchOutput);
                }
            }
        });
    });
    if let Some(message) = state.batch_message {
        ui.label(
            RichText::new(message)
                .small()
                .color(egui::Color32::from_rgb(255, 138, 112)),
        );
    }
    ui.label(
        RichText::new("each dataset gains object, track and colocalization tables")
            .small()
            .weak(),
    );
}
