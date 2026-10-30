use crate::analysis::Progress;
use crate::dataset::{Dataset, Origin};
use crate::overlay::{self, Overlay};
use crate::scene::Scene;
use crate::viewport::Viewport;
use egui::{RichText, Ui};
use grana_render::{ChannelDisplay, FrameInfo, Surface};
use grana_volume::Axis;

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
    pub view: &'a mut Viewport,
    pub adapter: &'a str,
    pub loading: Option<&'a str>,
    pub message: Option<&'a Message>,
    pub frame: FrameInfo,
    pub pending: usize,
    pub analysis: Option<Progress>,
}

#[derive(Clone, Copy)]
pub struct Actions {
    pub open: bool,
    pub fit: bool,
    pub fit_section: bool,
    pub section_axis: Option<Axis>,
    pub split: Option<f32>,
    pub detect_spots: bool,
    pub clear_spots: bool,
    pub cancel_analysis: bool,
    pub export_spots: bool,
    pub volume: egui::Rect,
    pub section: egui::Rect,
}

impl Actions {
    pub fn new() -> Self {
        Self {
            open: false,
            fit: false,
            fit_section: false,
            section_axis: None,
            split: None,
            detect_spots: false,
            clear_spots: false,
            cancel_analysis: false,
            export_spots: false,
            volume: egui::Rect::ZERO,
            section: egui::Rect::ZERO,
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
                    section_section(ui, state, &mut actions);
                    ui.separator();
                    channels_section(ui, state.scene);
                    ui.separator();
                    display_section(ui, state.scene);
                    ui.separator();
                    analysis_section(ui, state, &mut actions);
                    ui.separator();
                    rendering_section(ui, state.scene);
                    ui.separator();
                    streaming_section(ui, state);
                    ui.separator();
                    camera_section(ui, &mut actions);
                    ui.separator();
                    ui.label(RichText::new(state.adapter).small().weak());
                });
        });
    let rect = egui::CentralPanel::no_frame()
        .show(root, |ui| {
            let rect = ui.max_rect();
            let split = rect.left() + rect.width() * state.view.split();
            let handle = egui::Rect::from_min_max(
                egui::pos2(split - 3.0, rect.top()),
                egui::pos2(split + 3.0, rect.bottom()),
            );
            let response = ui.interact(
                handle,
                ui.make_persistent_id("grana.split"),
                egui::Sense::click_and_drag(),
            );
            if response.hovered() || response.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
            if response.dragged()
                && let Some(pointer) = response.interact_pointer_pos()
            {
                actions.split = Some(((pointer.x - rect.left()) / rect.width()).clamp(0.25, 0.8));
            }
            rect
        })
        .inner;
    let split = rect.left() + rect.width() * state.view.split();
    actions.volume =
        egui::Rect::from_min_max(rect.left_top(), egui::pos2(split - 1.0, rect.bottom()));
    actions.section =
        egui::Rect::from_min_max(egui::pos2(split + 1.0, rect.top()), rect.right_bottom());
    overlay::paint(
        root,
        &Overlay {
            camera: state.view.camera(),
            section: state.view.section(),
            world: state.scene.dataset.world_size(),
            levels: state.scene.dataset.level_count(),
            volume_level: state.frame.volume.level,
            section_level: state.frame.section.level,
            volume_rect: actions.volume,
            section_rect: actions.section,
        },
    );
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
            if matches!(view.display, ChannelDisplay::Surface(_)) {
                ui.label(RichText::new("surface").small().weak());
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
    let tint = scene.channel(index).tint();
    let data = scene.channel(index).intensity();
    let current = scene.channels[index].window;
    let mut display = scene.channels[index].display;
    let mut start = current.min();
    let mut end = current.max();
    let available = data.min()..=data.max();
    ui.strong(format!("Display · {name}"));
    ui.horizontal(|ui| {
        let surface = matches!(display, ChannelDisplay::Surface(_));
        if ui.selectable_label(!surface, "Volume").clicked() && surface {
            display = ChannelDisplay::Volume;
        }
        if ui.selectable_label(surface, "Surface").clicked() && !surface {
            display = ChannelDisplay::Surface(Surface::new(current.value(0.5), tint));
        }
    });
    let mut changed = false;
    changed |= ui
        .add(egui::Slider::new(&mut start, available.clone()).text("black"))
        .changed();
    changed |= ui
        .add(egui::Slider::new(&mut end, available.clone()).text("white"))
        .changed();
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(format!("data {:.0} … {:.0}", data.min(), data.max()))
                .small()
                .weak(),
        );
        if ui.small_button("Auto").clicked() {
            scene.reset_window(index);
            changed = false;
        }
    });
    if changed {
        scene.set_window(index, start, end);
    }
    match &mut display {
        ChannelDisplay::Surface(surface) => {
            let mut threshold = surface.threshold();
            if ui
                .add(egui::Slider::new(&mut threshold, available).text("threshold"))
                .changed()
            {
                surface.set_threshold(threshold);
            }
            ui.horizontal(|ui| {
                ui.label("surface color");
                let mut color = surface.color();
                if ui.color_edit_button_rgb(&mut color).changed() {
                    surface.set_color(color);
                }
            });
            ui.label(RichText::new("isosurface at the threshold").small().weak());
        }
        ChannelDisplay::Volume => {
            let window = scene.channels[index].window;
            scene.selected_transfer().ui(ui, window);
        }
    }
    scene.channels[index].display = display;
}

fn analysis_section(ui: &mut Ui, state: &mut UiState<'_>, actions: &mut Actions) {
    ui.strong("Objects");
    let selected = state.scene.selected;
    let name = state.scene.channel(selected).name().to_string();
    let minimum = state.scene.dataset.voxel_size().min_element();
    let mut params = state.scene.spot_params;
    let mut diameter = params.diameter();
    if ui
        .add(
            egui::Slider::new(&mut diameter, minimum..=16.0 * minimum)
                .logarithmic(true)
                .text("diameter")
                .suffix(" µm"),
        )
        .changed()
    {
        params.set_diameter(diameter);
    }
    let mut threshold = params.threshold();
    if ui
        .add(egui::Slider::new(&mut threshold, 0.0..=1.0).text("contrast"))
        .changed()
    {
        params.set_threshold(threshold);
    }
    state.scene.spot_params = params;

    ui.horizontal(|ui| {
        if ui
            .add_enabled(state.analysis.is_none(), egui::Button::new("Detect spots"))
            .clicked()
        {
            actions.detect_spots = true;
        }
        if state.analysis.is_some() {
            if ui.button("Cancel").clicked() {
                actions.cancel_analysis = true;
            }
            ui.spinner();
        }
        if let Some(progress) = state.analysis {
            if progress.total > 0 {
                ui.label(
                    RichText::new(format!(
                        "{} · {}/{} blocks",
                        state.scene.dataset.channels()[progress.channel].name(),
                        progress.done,
                        progress.total
                    ))
                    .small()
                    .weak(),
                );
            } else {
                ui.label(RichText::new("preparing").small().weak());
            }
        }
    });

    let summary = state.scene.spots().map(|spots| {
        (
            spots.channel(),
            spots.len(),
            spots.level(),
            spots.mean_quality(),
        )
    });
    if let Some((channel, count, level, mean)) = summary {
        let channel_name = state.scene.dataset.channels()[channel].name().to_string();
        ui.label(format!(
            "{channel_name} · {count} spots · level {}",
            level + 1
        ));
        if let Some(mean) = mean {
            ui.label(
                RichText::new(format!("mean contrast {mean:.2}"))
                    .small()
                    .weak(),
            );
        }
        let mut visible = state.scene.spot_visible();
        if ui.checkbox(&mut visible, "visible").changed() {
            state.scene.set_spot_visible(visible);
        }
        if ui.button("Export CSV").clicked() {
            actions.export_spots = true;
        }
        let picked = state.scene.selection();
        let measured = picked.and_then(|index| {
            let spot = state.scene.spot(index)?;
            Some((index, spot))
        });
        match measured {
            Some((index, spot)) => {
                ui.separator();
                egui::Grid::new("object")
                    .num_columns(2)
                    .spacing([12.0, 3.0])
                    .show(ui, |ui| {
                        let centre = spot.center();
                        row(ui, "object", format!("#{}", index + 1));
                        row(
                            ui,
                            "position",
                            format!("{:.2} · {:.2} · {:.2} µm", centre[0], centre[1], centre[2]),
                        );
                        row(ui, "diameter", format!("{:.3} µm", spot.radius() * 2.0));
                        row(ui, "contrast", format!("{:.3}", spot.quality()));
                    });
            }
            None => {
                ui.label(
                    RichText::new("click an object to measure it")
                        .small()
                        .weak(),
                );
            }
        }
    } else {
        ui.label(
            RichText::new(format!(
                "detects intensity maxima in {name} at the chosen diameter"
            ))
            .small()
            .weak(),
        );
    }
}

fn section_section(ui: &mut Ui, state: &mut UiState<'_>, actions: &mut Actions) {
    ui.strong("Section");
    let axis = state.view.section().axis();
    ui.horizontal(|ui| {
        for (name, candidate) in [("XY", Axis::Z), ("XZ", Axis::Y), ("YZ", Axis::X)] {
            if ui.selectable_label(axis == candidate, name).clicked() {
                actions.section_axis = Some(candidate);
            }
        }
        if ui.small_button("Fit").clicked() {
            actions.fit_section = true;
        }
    });
    let extent = state.scene.dataset.world_size()[axis.index()];
    let mut position = state.view.camera().focus_along(axis) + 0.5 * extent;
    if ui
        .add(egui::Slider::new(&mut position, 0.0..=extent).text("plane"))
        .changed()
    {
        state
            .view
            .camera_mut()
            .set_focus_along(axis, position - 0.5 * extent);
    }
    let spacing = state.scene.dataset.spacing();
    ui.label(
        RichText::new(format!(
            "{} plane · {:.3} µm steps",
            axis.name(),
            axis.spacing(spacing)
        ))
        .small()
        .weak(),
    );
    ui.label(
        RichText::new("drag pans · wheel steps · ctrl+wheel zooms · click selects")
            .small()
            .weak(),
    );
}

fn rendering_section(ui: &mut Ui, scene: &mut Scene) {
    ui.strong("Rendering");
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

fn streaming_section(ui: &mut Ui, state: &UiState<'_>) {
    ui.strong("Streaming");
    let levels = state.scene.dataset.level_count();
    let volume = state.frame.volume;
    let section = state.frame.section;
    egui::Grid::new("streaming")
        .num_columns(2)
        .spacing([12.0, 3.0])
        .show(ui, |ui| {
            row(
                ui,
                "volume",
                format!(
                    "level {} · {}/{} bricks",
                    volume.level + 1,
                    volume.resident,
                    volume.visible
                ),
            );
            row(
                ui,
                "section",
                format!(
                    "level {} · {}/{} bricks",
                    section.level + 1,
                    section.resident,
                    section.visible
                ),
            );
            row(ui, "pyramid", levels.to_string());
            row(
                ui,
                "cache",
                format!(
                    "{:.0} MiB of brick cache",
                    state.frame.bytes as f64 / (1024.0 * 1024.0)
                ),
            );
        });
    if state.pending > 0 {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(
                RichText::new(format!("{} bricks in flight", state.pending))
                    .small()
                    .weak(),
            );
        });
    }
}

fn camera_section(ui: &mut Ui, actions: &mut Actions) {
    ui.strong("View");
    if ui.button("Fit volume").clicked() {
        actions.fit = true;
    }
    ui.label(
        RichText::new("drag orbits · middle or right drag pans · wheel zooms · click selects")
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
