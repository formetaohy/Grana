use crate::analysis::{Job, Progress};
use crate::dataset::{Dataset, Origin};
use crate::objects::{ObjectKind, ObjectSetId};
use crate::overlay::{self, Overlay};
use crate::scene::{Scene, Selection};
use crate::statistics::Statistics;
use crate::viewport::Viewport;
use egui::{RichText, Ui};
use grana_render::FrameInfo;
use grana_volume::Axis;

const TIMELINE_HEIGHT: f32 = 36.0;

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
    pub statistics: &'a mut Statistics,
    pub adapter: &'a str,
    pub loading: Option<&'a str>,
    pub message: Option<&'a Message>,
    pub frame: FrameInfo,
    pub pending: usize,
    pub playing: bool,
    pub analysis: &'a [Progress],
}

#[derive(Clone, Copy)]
pub struct Actions {
    pub open: bool,
    pub fit: bool,
    pub fit_section: bool,
    pub section_axis: Option<Axis>,
    pub split: Option<f32>,
    pub time: Option<u32>,
    pub select: Option<Selection>,
    pub toggle_play: bool,
    pub add_object: Option<ObjectKind>,
    pub remove_object: Option<ObjectSetId>,
    pub detect: Option<ObjectSetId>,
    pub cancel_detect: Option<ObjectSetId>,
    pub clear_object: Option<ObjectSetId>,
    pub track: Option<ObjectSetId>,
    pub cancel_track: Option<ObjectSetId>,
    pub clear_tracks: Option<ObjectSetId>,
    pub export_statistics: bool,
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
            time: None,
            select: None,
            toggle_play: false,
            add_object: None,
            remove_object: None,
            detect: None,
            cancel_detect: None,
            clear_object: None,
            track: None,
            cancel_track: None,
            clear_tracks: None,
            export_statistics: false,
            volume: egui::Rect::ZERO,
            section: egui::Rect::ZERO,
        }
    }
}

pub fn build(root: &mut Ui, state: &mut UiState<'_>) -> Actions {
    let mut actions = Actions::new();
    if state.statistics.open() {
        egui::Panel::bottom("statistics")
            .resizable(true)
            .default_size(280.0)
            .min_size(280.0)
            .show(root, |ui| {
                crate::statistic_ui::build(ui, state, &mut actions);
            });
    }
    egui::Panel::left("controls")
        .exact_size(340.0)
        .resizable(true)
        .show(root, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("grana.controls")
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
                    crate::objects_ui::build(ui, state, &mut actions);
                    ui.separator();
                    statistics_section(ui, state);
                    ui.separator();
                    display_section(ui, state.scene);
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
    egui::CentralPanel::no_frame().show(root, |ui| {
        let full = ui.max_rect();
        let timeline = (state.scene.frame_count() > 1).then(|| {
            egui::Rect::from_min_max(
                egui::pos2(full.left(), full.bottom() - TIMELINE_HEIGHT),
                full.right_bottom(),
            )
        });
        let panes = match timeline {
            Some(bar) => {
                egui::Rect::from_min_max(full.left_top(), egui::pos2(full.right(), bar.top()))
            }
            None => full,
        };
        let split = panes.left() + panes.width() * state.view.split();
        let handle = egui::Rect::from_min_max(
            egui::pos2(split - 3.0, panes.top()),
            egui::pos2(split + 3.0, panes.bottom()),
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
            actions.split = Some(((pointer.x - panes.left()) / panes.width()).clamp(0.25, 0.8));
        }
        actions.volume =
            egui::Rect::from_min_max(panes.left_top(), egui::pos2(split - 1.0, panes.bottom()));
        actions.section =
            egui::Rect::from_min_max(egui::pos2(split + 1.0, panes.top()), panes.right_bottom());
        overlay::paint(
            ui,
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
        if let Some(bar) = timeline {
            timeline_bar(ui, bar, state, &mut actions);
        }
    });
    actions
}

fn timeline_bar(ui: &mut Ui, rect: egui::Rect, state: &UiState<'_>, actions: &mut Actions) {
    ui.painter().line_segment(
        [
            egui::pos2(rect.left(), rect.top()),
            egui::pos2(rect.right(), rect.top()),
        ],
        egui::Stroke::new(1.0, egui::Color32::from_gray(44)),
    );
    ui.scope_builder(
        egui::UiBuilder::new().max_rect(rect.shrink2(egui::vec2(12.0, 3.0))),
        |ui| {
            ui.horizontal_centered(|ui| {
                let label = if state.playing { "Pause" } else { "Play" };
                if ui.button(label).clicked() {
                    actions.toggle_play = true;
                }
                let frames = state.scene.frame_count();
                let mut time = state.scene.time();
                let width = (ui.available_width() - 148.0).max(80.0);
                let slider = egui::Slider::new(&mut time, 0..=(frames - 1)).show_value(false);
                if ui.add_sized([width, 18.0], slider).changed() {
                    actions.time = Some(time);
                }
                let seconds = time as f32 * state.scene.dataset.frame_interval();
                ui.monospace(format!("{:>3} / {} · {seconds:.1} s", time + 1, frames));
            });
        },
    );
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
            row(ui, "frames", dataset.frame_count().to_string());
            if dataset.frame_count() > 1 {
                row(
                    ui,
                    "frame interval",
                    format!("{:.3} s", dataset.frame_interval()),
                );
            }
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

fn statistics_section(ui: &mut Ui, state: &mut UiState<'_>) {
    ui.horizontal(|ui| {
        if ui
            .selectable_label(state.statistics.open(), "Statistics")
            .clicked()
        {
            state.statistics.set_open(!state.statistics.open());
        }
        ui.label(
            RichText::new("measure objects across channels")
                .small()
                .weak(),
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
        scene.select_channel(index);
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
    let window = scene.channels[index].window;
    scene.selected_transfer().ui(ui, window);
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

pub(crate) fn progress(analysis: &[Progress], set: ObjectSetId, job: Job) -> Option<Progress> {
    analysis
        .iter()
        .copied()
        .find(|progress| progress.set == set && progress.job == job)
}

pub(crate) fn caption(kind: ObjectKind, job: Job, progress: Progress, frames: u32) -> String {
    let unit = match (job, kind) {
        (Job::Detect, ObjectKind::Spots) => "blocks",
        (Job::Detect, ObjectKind::Surfaces) => "bricks",
        (Job::Track, _) => "frames",
    };
    let prefix = match job {
        Job::Detect if frames > 1 => format!("frame {} · ", progress.time + 1),
        Job::Detect | Job::Track => String::new(),
    };
    if progress.total > 0 {
        format!("{prefix}{}/{} {unit}", progress.done, progress.total)
    } else {
        format!("{prefix}preparing")
    }
}

pub(crate) fn color_swatch(ui: &mut Ui, tint: [f32; 3]) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(16.0, 12.0), egui::Sense::hover());
    let color = egui::Color32::from_rgb(to_byte(tint[0]), to_byte(tint[1]), to_byte(tint[2]));
    ui.painter().rect_filled(rect, 2.0, color);
}

pub(crate) fn row(ui: &mut Ui, name: &str, value: String) {
    ui.label(RichText::new(name).weak());
    ui.monospace(value);
    ui.end_row();
}

fn to_byte(channel: f32) -> u8 {
    (channel.clamp(0.0, 1.0) * 255.0).round() as u8
}
