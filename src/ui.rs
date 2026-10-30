use crate::dataset::Dataset;
use crate::transfer_editor::TransferEditor;
use egui::{RichText, Ui};
use grana_render::RenderParams;

pub struct UiState<'a> {
    pub transfer: &'a mut TransferEditor,
    pub params: &'a mut RenderParams,
    pub dataset: &'a Dataset,
    pub adapter: &'a str,
}

#[derive(Clone, Copy)]
pub struct Actions {
    pub fit: bool,
    pub viewport: egui::Rect,
}

impl Actions {
    pub fn new() -> Self {
        Self {
            fit: false,
            viewport: egui::Rect::ZERO,
        }
    }
}

pub fn build(root: &mut Ui, state: &mut UiState<'_>) -> Actions {
    let mut actions = Actions::new();
    egui::Panel::left("controls")
        .exact_size(330.0)
        .resizable(true)
        .show(root, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.heading("grana");
                ui.label(RichText::new("microscopy volume viewer").small().weak());
                ui.separator();

                dataset_section(ui, state.dataset);
                ui.separator();

                ui.strong("Transfer function");
                ui.add_space(2.0);
                state.transfer.ui(ui, state.dataset.intensity);
                ui.separator();

                rendering_section(ui, state.params);
                ui.separator();

                ui.strong("Camera");
                if ui.button("Fit volume").clicked() {
                    actions.fit = true;
                }
                ui.label(
                    RichText::new("drag orbits · middle or right drag pans · wheel zooms")
                        .small()
                        .weak(),
                );
                ui.separator();
                ui.label(RichText::new(state.adapter).small().weak());
            });
        });
    actions.viewport = egui::CentralPanel::no_frame()
        .show(root, |ui| ui.max_rect())
        .inner;
    actions
}

fn dataset_section(ui: &mut Ui, dataset: &Dataset) {
    ui.strong(&dataset.name);
    egui::Grid::new("dataset")
        .num_columns(2)
        .spacing([12.0, 3.0])
        .show(ui, |ui| {
            let extent = dataset.extent;
            row(
                ui,
                "dimensions",
                format!("{} × {} × {}", extent.x(), extent.y(), extent.z()),
            );
            let spacing = dataset.spacing;
            row(
                ui,
                "voxel size",
                format!(
                    "{:.3} × {:.3} × {:.3} µm",
                    spacing.x(),
                    spacing.y(),
                    spacing.z()
                ),
            );
            let world = dataset.world_size;
            row(
                ui,
                "field of view",
                format!("{:.1} × {:.1} × {:.1} µm", world.x, world.y, world.z),
            );
            row(ui, "format", format!("{:?}", dataset.format));
            row(
                ui,
                "intensity",
                format!(
                    "{:.0} … {:.0}",
                    dataset.intensity.min(),
                    dataset.intensity.max()
                ),
            );
            row(ui, "levels", dataset.level_count.to_string());
        });
}

fn rendering_section(ui: &mut Ui, params: &mut RenderParams) {
    ui.strong("Volume");
    ui.add(
        egui::Slider::new(&mut params.step_scale, 0.25..=2.0)
            .text("sampling distance")
            .logarithmic(true),
    );
    ui.add(egui::Slider::new(&mut params.opacity, 0.0..=1.0).text("opacity"));
    ui.add(egui::Slider::new(&mut params.shading.ambient, 0.0..=1.0).text("ambient"));
    ui.add(egui::Slider::new(&mut params.shading.diffuse, 0.0..=1.0).text("diffuse"));
    ui.add(egui::Slider::new(&mut params.shading.specular, 0.0..=1.0).text("specular"));
    ui.add(
        egui::Slider::new(&mut params.shading.shininess, 1.0..=128.0)
            .text("shininess")
            .logarithmic(true),
    );
}

fn row(ui: &mut Ui, name: &str, value: String) {
    ui.label(RichText::new(name).weak());
    ui.monospace(value);
    ui.end_row();
}
