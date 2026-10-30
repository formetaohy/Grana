use crate::dataset::{Dataset, Origin};
use crate::scene::Scene;
use egui::{RichText, Ui};
use grana_render::FrameInfo;

pub struct Message(String);

impl Message {
    pub fn error(message: impl Into<String>) -> Self {
        Self(message.into())
    }

    pub fn text(&self) -> &str {
        &self.0
    }
}

pub struct UiState<'a> {
    pub scene: &'a mut Scene,
    pub adapter: &'a str,
    pub loading: Option<&'a str>,
    pub message: Option<&'a Message>,
    pub frame: FrameInfo,
    pub pending: usize,
}

#[derive(Clone, Copy)]
pub struct Actions {
    pub open: bool,
    pub fit: bool,
    pub viewport: egui::Rect,
}

impl Actions {
    pub fn new() -> Self {
        Self {
            open: false,
            fit: false,
            viewport: egui::Rect::ZERO,
        }
    }
}

pub fn build(root: &mut Ui, state: &mut UiState<'_>) -> Actions {
    let mut actions = Actions::new();
    egui::Panel::left("controls")
        .exact_size(340.0)
        .resizable(true)
        .show(root, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.heading("grana");
                    ui.label(RichText::new("microscopy volume workspace").small().weak());
                    ui.separator();
                    open_section(ui, &mut actions, state.loading, state.message);
                    ui.separator();
                    dataset_section(ui, &state.scene.dataset);
                    ui.separator();
                    channels_section(ui, state.scene);
                    ui.separator();
                    display_section(ui, state.scene);
                    ui.separator();
                    rendering_section(ui, state.scene);
                    ui.separator();
                    streaming_section(
                        ui,
                        state.frame,
                        state.pending,
                        state.scene.dataset.level_count(),
                    );
                    ui.separator();
                    camera_section(ui, &mut actions);
                    ui.separator();
                    ui.label(RichText::new(state.adapter).small().weak());
                });
        });
    actions.viewport = egui::CentralPanel::no_frame()
        .show(root, |ui| ui.max_rect())
        .inner;
    actions
}

fn open_section(
    ui: &mut Ui,
    actions: &mut Actions,
    loading: Option<&str>,
    message: Option<&Message>,
) {
    ui.horizontal(|ui| {
        if ui
            .add_enabled(loading.is_none(), egui::Button::new("Open OME-Zarr"))
            .clicked()
        {
            actions.open = true;
        }
        if let Some(name) = loading {
            ui.spinner();
            ui.label(RichText::new(name).small().weak());
        }
    });
    ui.label(
        RichText::new("streams bricks of a multiscale .zarr folder")
            .small()
            .weak(),
    );
    if let Some(message) = message {
        ui.label(
            RichText::new(message.text())
                .color(egui::Color32::from_rgb(255, 138, 112))
                .small(),
        );
    }
}

fn dataset_section(ui: &mut Ui, dataset: &Dataset) {
    ui.strong(dataset.name());
    egui::Grid::new("dataset")
        .num_columns(2)
        .spacing([12.0, 3.0])
        .show(ui, |ui| {
            let extent = dataset.extent();
            row(
                ui,
                "dimensions",
                format!("{} × {} × {}", extent.x(), extent.y(), extent.z()),
            );
            let spacing = dataset.spacing();
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
            let world = dataset.world_size();
            row(
                ui,
                "field of view",
                format!("{:.1} × {:.1} × {:.1} µm", world.x, world.y, world.z),
            );
            row(ui, "channels", dataset.channels().len().to_string());
            row(
                ui,
                "format",
                dataset
                    .format()
                    .map_or_else(|| "mixed".to_string(), |format| format!("{format:?}")),
            );
            row(ui, "pyramid", dataset.level_count().to_string());
            row(
                ui,
                "source",
                match dataset.origin() {
                    Origin::Synthetic => "synthetic".to_string(),
                    Origin::Zarr => "OME-Zarr".to_string(),
                },
            );
        });
}

fn channels_section(ui: &mut Ui, scene: &mut Scene) {
    ui.strong("Channels");
    let selected = scene.selected;
    let mut clicked = None;
    for (index, view) in scene.channels.iter_mut().enumerate() {
        let channel = &scene.dataset.channels()[index];
        ui.horizontal(|ui| {
            ui.checkbox(&mut view.enabled, "");
            color_swatch(ui, channel.tint());
            if ui
                .selectable_label(selected == index, channel.name())
                .clicked()
            {
                clicked = Some(index);
            }
        });
        ui.add(
            egui::Slider::new(&mut view.opacity, 0.0..=1.0)
                .text("opacity")
                .show_value(false),
        );
    }
    if let Some(index) = clicked {
        scene.select(index);
    }
}

fn display_section(ui: &mut Ui, scene: &mut Scene) {
    let index = scene.selected;
    let name = scene.channel(index).name().to_string();
    let data = scene.channel(index).intensity();
    let current = scene.channels[index].window;
    let mut start = current.min();
    let mut end = current.max();
    let available = data.min()..=data.max();
    ui.strong(format!("Display · {name}"));
    let mut changed = false;
    changed |= ui
        .add(egui::Slider::new(&mut start, available.clone()).text("black"))
        .changed();
    changed |= ui
        .add(egui::Slider::new(&mut end, available).text("white"))
        .changed();
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(format!("data {:.0} … {:.0}", data.min(), data.max()))
                .small()
                .weak(),
        );
        if ui.small_button("Auto").clicked() {
            scene.reset_window(index);
        }
    });
    if changed {
        scene.set_window(index, start, end);
    }
    let window = scene.channels[index].window;
    scene.selected_transfer().ui(ui, window);
}

fn rendering_section(ui: &mut Ui, scene: &mut Scene) {
    ui.strong("Volume");
    ui.add(
        egui::Slider::new(&mut scene.step_scale, 0.25..=2.0)
            .text("sampling distance")
            .logarithmic(true),
    );
    let shading = &mut scene.shading;
    ui.add(egui::Slider::new(&mut shading.ambient, 0.0..=1.0).text("ambient"));
    ui.add(egui::Slider::new(&mut shading.diffuse, 0.0..=1.0).text("diffuse"));
    ui.add(egui::Slider::new(&mut shading.specular, 0.0..=1.0).text("specular"));
    ui.add(
        egui::Slider::new(&mut shading.shininess, 1.0..=128.0)
            .text("shininess")
            .logarithmic(true),
    );
}

fn streaming_section(ui: &mut Ui, frame: FrameInfo, pending: usize, levels: usize) {
    ui.strong("Streaming");
    egui::Grid::new("streaming")
        .num_columns(2)
        .spacing([12.0, 3.0])
        .show(ui, |ui| {
            row(ui, "level", format!("{} of {levels}", frame.level + 1));
            row(
                ui,
                "resident",
                format!("{} / {} bricks", frame.resident, frame.visible),
            );
            row(
                ui,
                "cache",
                format!(
                    "{:.0} MiB · {} slots",
                    frame.bytes as f64 / (1024.0 * 1024.0),
                    frame.slots
                ),
            );
        });
    if pending > 0 {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(
                RichText::new(format!("{pending} bricks in flight"))
                    .small()
                    .weak(),
            );
        });
    }
}

fn camera_section(ui: &mut Ui, actions: &mut Actions) {
    ui.strong("Camera");
    if ui.button("Fit volume").clicked() {
        actions.fit = true;
    }
    ui.label(
        RichText::new("drag orbits · middle or right drag pans · wheel zooms")
            .small()
            .weak(),
    );
}

fn color_swatch(ui: &mut Ui, tint: [f32; 3]) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(16.0, 12.0), egui::Sense::hover());
    let color = egui::Color32::from_rgb(to_byte(tint[0]), to_byte(tint[1]), to_byte(tint[2]));
    ui.painter().rect_filled(rect, 2.0, color);
}

fn row(ui: &mut Ui, name: &str, value: String) {
    ui.label(RichText::new(name).weak());
    ui.monospace(value);
    ui.end_row();
}

fn to_byte(channel: f32) -> u8 {
    (channel.clamp(0.0, 1.0) * 255.0).round() as u8
}
