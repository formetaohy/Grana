use grana_ui::{Align, Axis, Block, Color, Length, Role, TextStyle, Ui};

use crate::action::Action;
use crate::state::State;
use crate::widget;

pub fn build(ui: &mut Ui, state: &mut State<'_>, actions: &mut Vec<Action>) {
    widget::caption(ui, "run one protocol over many OME-Zarr datasets");
    protocol(ui, state, actions);
    datasets(ui, state, actions);
    output(ui, state, actions);
}

fn protocol(ui: &mut Ui, state: &State<'_>, actions: &mut Vec<Action>) {
    let running = state.batch_running;
    widget::section_title(ui, "protocol");
    ui.row(Block::row().gaps(8.0).align(Align::Center), |ui| {
        if !running && widget::button(ui, "capture", "Capture", Role::Prominent) {
            actions.push(Action::CaptureProtocol);
        }
        if !running && widget::button(ui, "load", "Load", Role::Quiet) {
            actions.push(Action::LoadProtocol);
        }
        if !running && widget::button(ui, "save", "Save", Role::Quiet) {
            actions.push(Action::SaveProtocol);
        }
    });
    match state.batch_protocol {
        Some(text) => {
            let style = TextStyle::new(
                ui.theme().font(grana_ui::Level::Monospace),
                ui.theme().palette.text_secondary,
            );
            ui.text_style_wrapped(text, style, ui.available().x);
        }
        None => widget::weak(
            ui,
            "capture the current objects, parameters and colocalization pair",
        ),
    }
}

fn datasets(ui: &mut Ui, state: &State<'_>, actions: &mut Vec<Action>) {
    let running = state.batch_running;
    ui.row(Block::row().gaps(10.0).align(Align::Center), |ui| {
        widget::section_title(ui, "datasets");
        widget::caption(ui, &format!("{}", state.batch_datasets.len()));
    });
    if state.batch_datasets.is_empty() {
        widget::weak(ui, "a folder of OME-Zarr datasets that share this protocol");
    } else {
        let key = ui.id("batch.datasets");
        ui.scroll_axis(key, Axis::Column, Length::Fixed(112.0), None, |ui| {
            for dataset in state.batch_datasets {
                ui.row(Block::row().gaps(10.0).align(Align::Center), |ui| {
                    let style = TextStyle::new(
                        ui.theme().font(grana_ui::Level::Monospace),
                        ui.theme().palette.text_primary,
                    );
                    ui.text_style(&dataset.name, style);
                    widget::caption(ui, &dataset.state);
                    if !dataset.detail.is_empty() {
                        widget::weak(ui, &dataset.detail);
                    }
                });
            }
        });
    }
    ui.row(Block::row().gaps(8.0).align(Align::Center), |ui| {
        if !running && widget::button(ui, "add", "Add folder", Role::Control) {
            actions.push(Action::AddBatchDatasets);
        }
        if !running
            && !state.batch_datasets.is_empty()
            && widget::button(ui, "clear", "Clear", Role::Quiet)
        {
            actions.push(Action::ClearBatchDatasets);
        }
    });
}

fn output(ui: &mut Ui, state: &State<'_>, actions: &mut Vec<Action>) {
    widget::section_title(ui, "output");
    match state.batch_output {
        Some(path) => widget::weak(ui, path),
        None => widget::weak(ui, "choose a folder for the statistics"),
    }
    ui.row(Block::row().gaps(10.0).align(Align::Center), |ui| {
        if state.batch_running {
            if widget::button(ui, "cancel", "Cancel", Role::Quiet) {
                actions.push(Action::CancelBatch);
            }
            widget::spinner(ui, 14.0);
        } else {
            let ready = state.batch_protocol.is_some()
                && !state.batch_datasets.is_empty()
                && state.batch_output.is_some();
            if widget::button(ui, "output", "Choose", Role::Control) {
                actions.push(Action::ChooseBatchOutput);
            }
            if ready && widget::button(ui, "run", "Run", Role::Prominent) {
                actions.push(Action::RunBatch);
            }
        }
    });
    if let Some(message) = state.batch_message {
        let style = TextStyle::new(
            ui.theme().font(grana_ui::Level::Caption),
            Color::rgb(1.0, 0.53, 0.44),
        );
        ui.text_style_wrapped(message, style, ui.available().x);
    }
    widget::weak(
        ui,
        "each dataset gains object, track and colocalization tables",
    );
}
