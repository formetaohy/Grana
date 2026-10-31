use egui::{RichText, Ui};
use grana_render::ViewInfo;
use grana_scene::{Dataset, Job, ObjectKind, Progress, Scene};
use grana_volume::Axis;

use crate::action::Action;
use crate::overlay::{self, Overlay};
use crate::state::State;

const TIMELINE_HEIGHT: f32 = 36.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Panel {
    Statistics,
    Colocalization,
}

impl Panel {
    pub fn name(self) -> &'static str {
        match self {
            Self::Statistics => "Statistics",
            Self::Colocalization => "Colocalization",
        }
    }
}

pub fn build(root: &mut Ui, state: &mut State<'_>) -> Vec<Action> {
    let mut actions = Vec::new();
    if let Some(panel) = *state.panel {
        egui::Panel::bottom("analysis")
            .resizable(true)
            .default_size(300.0)
            .min_size(220.0)
            .show(root, |ui| {
                tabs(ui, &mut actions, panel);
                ui.separator();
                match panel {
                    Panel::Statistics => crate::measure_panel::build(ui, state, &mut actions),
                    Panel::Colocalization => crate::coloc_panel::build(ui, state, &mut actions),
                }
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
                    file_section(ui, &mut actions, state);
                    ui.separator();
                    dataset_section(ui, &state.scene.dataset);
                    ui.separator();
                    section_section(ui, state, &mut actions);
                    ui.separator();
                    region_section(ui, state);
                    ui.separator();
                    channels_section(ui, state.scene);
                    ui.separator();
                    crate::object_panel::build(ui, state, &mut actions);
                    ui.separator();
                    measure_section(ui, state, &mut actions);
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
        let split = panes.left() + panes.width() * state.viewport.split();
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
            actions.push(Action::SetSplit(
                ((pointer.x - panes.left()) / panes.width()).clamp(0.25, 0.8),
            ));
        }
        let volume =
            egui::Rect::from_min_max(panes.left_top(), egui::pos2(split - 1.0, panes.bottom()));
        let section =
            egui::Rect::from_min_max(egui::pos2(split + 1.0, panes.top()), panes.right_bottom());
        state.viewport.set_rects(volume, section);
        let world = state.scene.dataset.world_size();
        let levels = state.scene.dataset.level_count();
        let mut overlay = Overlay {
            camera: state.viewport.camera(),
            section: state.viewport.section(),
            world,
            levels,
            volume_level: state.frame.volume.level,
            section_level: state.frame.section.level,
            volume_rect: volume,
            section_rect: section,
            region: &mut state.scene.region,
        };
        overlay::paint(ui, &mut overlay);
        if let Some(bar) = timeline {
            timeline_bar(ui, bar, state, &mut actions);
        }
    });
    actions
}

fn tabs(ui: &mut Ui, actions: &mut Vec<Action>, panel: Panel) {
    ui.horizontal(|ui| {
        for candidate in [Panel::Statistics, Panel::Colocalization] {
            if ui
                .selectable_label(panel == candidate, candidate.name())
                .clicked()
            {
                actions.push(Action::SetPanel(Some(candidate)));
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Hide").clicked() {
                actions.push(Action::SetPanel(None));
            }
        });
    });
}
fn timeline_bar(ui: &mut Ui, rect: egui::Rect, state: &State<'_>, actions: &mut Vec<Action>) {
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
                    actions.push(Action::TogglePlay);
                }
                let frames = state.scene.frame_count();
                let mut time = state.scene.time();
                let width = (ui.available_width() - 148.0).max(80.0);
                let slider = egui::Slider::new(&mut time, 0..=(frames - 1)).show_value(false);
                if ui.add_sized([width, 18.0], slider).changed() {
                    actions.push(Action::SetTime(time));
                }
                let seconds = time as f32 * state.scene.dataset.frame_interval();
                ui.monospace(format!("{:>3} / {} · {seconds:.1} s", time + 1, frames));
            });
        },
    );
}

fn file_section(ui: &mut Ui, actions: &mut Vec<Action>, state: &State<'_>) {
    let busy = state.loading.is_some();
    ui.horizontal(|ui| {
        if ui
            .add_enabled(!busy, egui::Button::new("Open dataset"))
            .clicked()
        {
            actions.push(Action::OpenDataset);
        }
        if ui
            .add_enabled(!busy, egui::Button::new("Open project"))
            .clicked()
        {
            actions.push(Action::OpenProject);
        }
    });
    ui.horizontal(|ui| {
        if ui
            .add_enabled(!busy, egui::Button::new("Save project"))
            .clicked()
        {
            actions.push(Action::SaveProject);
        }
        if ui
            .add_enabled(!busy, egui::Button::new("Save as"))
            .clicked()
        {
            actions.push(Action::SaveProjectAs);
        }
    });
    match state.project {
        Some(name) => {
            let marker = if state.dirty { " •" } else { "" };
            ui.label(RichText::new(format!("{name}{marker}")).small());
        }
        None => {
            ui.label(
                RichText::new("an untitled project: save it to keep every analysis")
                    .small()
                    .weak(),
            );
        }
    }
    if let Some(name) = state.loading {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(RichText::new(name).small().weak());
        });
    }
    if let Some(message) = state.message {
        ui.label(
            RichText::new(message.text())
                .color(egui::Color32::from_rgb(255, 138, 112))
                .small(),
        );
    }
    ui.label(
        RichText::new("streams bricks of a multiscale .zarr folder into a .grana project")
            .small()
            .weak(),
    );
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
                    grana_scene::Origin::Synthetic => "synthetic".to_string(),
                    grana_scene::Origin::Zarr => "OME-Zarr".to_string(),
                },
            );
        });
}

fn measure_section(ui: &mut Ui, state: &State<'_>, actions: &mut Vec<Action>) {
    ui.horizontal(|ui| {
        let open = state.panel.is_some();
        if ui.selectable_label(open, "Analysis").clicked() {
            actions.push(Action::SetPanel((!open).then_some(Panel::Statistics)));
        }
        ui.label(
            RichText::new("statistics and colocalization")
                .small()
                .weak(),
        );
    });
}

fn channels_section(ui: &mut Ui, scene: &mut Scene) {
    ui.strong("Channels");
    let selected = scene.selected;
    let mut clicked = None;
    for (index, channel) in scene.channels.iter_mut().enumerate() {
        let info = &scene.dataset.channels()[index];
        ui.horizontal(|ui| {
            ui.checkbox(&mut channel.enabled, "");
            color_swatch(ui, info.tint());
            if ui
                .selectable_label(selected == index, info.name())
                .clicked()
            {
                clicked = Some(index);
            }
        });
        ui.add(
            egui::Slider::new(&mut channel.opacity, 0.0..=1.0)
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
    let tint = scene.channel(index).tint();
    crate::transfer::edit(ui, scene.channels[index].transfer_mut(), tint, window);
}

fn section_section(ui: &mut Ui, state: &mut State<'_>, actions: &mut Vec<Action>) {
    ui.strong("Section");
    let axis = state.viewport.section().axis();
    ui.horizontal(|ui| {
        for (name, candidate) in [("XY", Axis::Z), ("XZ", Axis::Y), ("YZ", Axis::X)] {
            if ui.selectable_label(axis == candidate, name).clicked() {
                actions.push(Action::SetSectionAxis(candidate));
            }
        }
        if ui.small_button("Fit").clicked() {
            actions.push(Action::FitSection);
        }
    });
    let extent = state.scene.dataset.world_size()[axis.index()];
    let mut position = state.viewport.camera().focus_along(axis) + 0.5 * extent;
    if ui
        .add(egui::Slider::new(&mut position, 0.0..=extent).text("plane"))
        .changed()
    {
        state
            .viewport
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

fn region_section(ui: &mut Ui, state: &mut State<'_>) {
    ui.strong("Region");
    let world = state.scene.dataset.world_size();
    let half = 0.5 * world;
    let mut region = state.scene.region;
    let mut enabled = region.enabled();
    ui.horizontal(|ui| {
        if ui
            .checkbox(&mut enabled, "limit analysis and display")
            .changed()
        {
            region.set_enabled(enabled);
        }
        if ui.small_button("Reset").clicked() {
            region.set_bounds(-half, half);
        }
    });
    ui.label(
        RichText::new(if enabled {
            "detection, tracking and picking stay inside the box; re-detect after moving it"
        } else {
            "detection, tracking and picking cover the whole dataset"
        })
        .small()
        .weak(),
    );
    let minimum = (0.001 * world.min_element()).max(1.0e-4);
    let mut low = region.min();
    let mut high = region.max();
    ui.add_enabled_ui(enabled, |ui| {
        for (axis, name) in [(Axis::X, "x"), (Axis::Y, "y"), (Axis::Z, "z")] {
            let index = axis.index();
            let limit = half[index];
            let speed = (0.01 * world[index]).max(minimum);
            ui.horizontal(|ui| {
                ui.label(name);
                ui.spacing_mut().slider_width = 56.0;
                let mut start = low[index];
                let mut end = high[index];
                let changed = ui
                    .add(egui::Slider::new(&mut start, -limit..=limit).show_value(false))
                    .changed()
                    | ui.add(egui::Slider::new(&mut end, -limit..=limit).show_value(false))
                        .changed()
                    | ui.add(
                        egui::DragValue::new(&mut start)
                            .speed(speed)
                            .max_decimals(3),
                    )
                    .changed()
                    | ui.add(egui::DragValue::new(&mut end).speed(speed).max_decimals(3))
                        .changed();
                if changed {
                    start = start.clamp(-limit, (end - minimum).max(-limit));
                    end = end.clamp((start + minimum).min(limit), limit);
                    low[index] = start;
                    high[index] = end;
                }
                ui.label(RichText::new("µm").small().weak());
            });
        }
    });
    if low != region.min() || high != region.max() {
        region.set_bounds(low, high);
    }
    if enabled {
        let size = region.size();
        ui.label(
            RichText::new(format!("{:.2} × {:.2} × {:.2} µm", size.x, size.y, size.z))
                .small()
                .weak(),
        );
    }
    state.scene.region = region;
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
    ui.add(egui::Slider::new(&mut shading.shadows, 0.0..=1.0).text("shadows"));
}

fn streaming_section(ui: &mut Ui, state: &State<'_>) {
    ui.strong("Streaming");
    let levels = state.scene.dataset.level_count();
    let volume = state.frame.volume;
    let section = state.frame.section;
    egui::Grid::new("streaming")
        .num_columns(2)
        .spacing([12.0, 3.0])
        .show(ui, |ui| {
            row(ui, "volume", bricks(volume));
            row(ui, "section", bricks(section));
            row(ui, "pyramid", levels.to_string());
            row(
                ui,
                "cache",
                format!(
                    "{:.0} MiB of brick cache",
                    state.frame.bytes as f64 / (1024.0 * 1024.0)
                ),
            );
            row(
                ui,
                "geometry",
                format!(
                    "{:.0} MiB of packed geometry",
                    state.scene.geometry_bytes() as f64 / (1024.0 * 1024.0)
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

fn bricks(view: ViewInfo) -> String {
    if view.demand == view.visible {
        format!(
            "level {} · {}/{} bricks",
            view.level + 1,
            view.resident,
            view.demand
        )
    } else {
        format!(
            "level {} · {}/{} of {} bricks",
            view.level + 1,
            view.resident,
            view.demand,
            view.visible
        )
    }
}

fn camera_section(ui: &mut Ui, actions: &mut Vec<Action>) {
    ui.strong("View");
    if ui.button("Fit volume").clicked() {
        actions.push(Action::Fit);
    }
    ui.label(
        RichText::new("drag orbits · middle or right drag pans · wheel zooms · click selects")
            .small()
            .weak(),
    );
}

pub(crate) fn progress(analysis: &[Progress], job: Job) -> Option<Progress> {
    analysis
        .iter()
        .copied()
        .find(|progress| progress.job == job)
}

pub(crate) fn caption(kind: ObjectKind, job: Job, progress: Progress, frames: u32) -> String {
    let unit = match job {
        Job::Detect(_) => match kind {
            ObjectKind::Spots => "blocks",
            ObjectKind::Surfaces => "bricks",
            ObjectKind::Filaments => "frames",
        },
        Job::Track(_) | Job::Coloc => "frames",
    };
    let prefix = match job {
        Job::Detect(_) if frames > 1 => format!("frame {} · ", progress.time + 1),
        Job::Detect(_) | Job::Track(_) | Job::Coloc => String::new(),
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
