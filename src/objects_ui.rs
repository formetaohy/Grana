use egui::{RichText, Ui};

use crate::analysis::Job;
use crate::objects::{ObjectFrame, ObjectKind, ObjectParams, ObjectSet, ObjectSetId};
use crate::scene::{Item, Selection};
use crate::ui::{Actions, UiState, caption, progress, row};

pub fn build(ui: &mut Ui, state: &mut UiState<'_>, actions: &mut Actions) {
    ui.strong("Objects");
    let channel = state.scene.selected;
    let channel_name = state.scene.channel(channel).name().to_string();
    let loading = state.loading.is_some();
    ui.horizontal(|ui| {
        if ui
            .add_enabled(!loading, egui::Button::new("+ Spots"))
            .clicked()
        {
            actions.add_object = Some(ObjectKind::Spots);
        }
        if ui
            .add_enabled(!loading, egui::Button::new("+ Surfaces"))
            .clicked()
        {
            actions.add_object = Some(ObjectKind::Surfaces);
        }
    });
    ui.label(
        RichText::new(format!("new objects read {channel_name}"))
            .small()
            .weak(),
    );
    if state.scene.objects.len() == 0 {
        ui.label(
            RichText::new("add an object, then detect and track it")
                .small()
                .weak(),
        );
        return;
    }
    let ids = state
        .scene
        .objects
        .iter()
        .map(ObjectSet::id)
        .collect::<Vec<_>>();
    for id in ids {
        ui.add_space(6.0);
        object(ui, state, actions, id);
    }
}

fn object(ui: &mut Ui, state: &mut UiState<'_>, actions: &mut Actions, id: ObjectSetId) {
    let Some(set) = state.scene.objects.get(id) else {
        return;
    };
    let kind = set.kind();
    let name = set.name().to_string();
    let channel = set.channel();
    let channel_name = state.scene.dataset.channels()[channel].name().to_string();
    let mut color = set.color();
    let mut visible = set.visible();
    let detected = set.detected();
    let frames = state.scene.frame_count();
    let time = state.scene.time();
    let current = set
        .frame(time)
        .map(|frame| (frame.object_count(), frame.level()));
    let mean = set
        .frame(time)
        .and_then(ObjectFrame::spots)
        .and_then(|spots| spots.mean_quality());
    let mut remove = false;
    let summary = Summary {
        kind,
        channel: channel_name.clone(),
        current,
        mean,
        detected,
        frames,
        time,
    };

    let header_id = ui.make_persistent_id(("grana.object", id.raw()));
    let collapse =
        egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), header_id, true);
    let header = collapse.show_header(ui, |ui| {
        ui.color_edit_button_rgb(&mut color);
        ui.label(RichText::new(&name).strong());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("Remove").clicked() {
                remove = true;
            }
            let mut shown = visible;
            if ui
                .checkbox(&mut shown, "")
                .on_hover_text("visible")
                .changed()
            {
                visible = shown;
            }
        });
    });
    header.body(|ui| {
        ui.label(RichText::new(&channel_name).small().weak());
        params(ui, state, id);
        detect(ui, state, actions, id, kind, frames);
        summary_rows(ui, &summary);
        ui.horizontal(|ui| {
            if ui.button("Clear").clicked() {
                actions.clear_object = Some(id);
            }
        });
        measure(ui, state, id, time);
        crate::track_ui::build(ui, state, actions, id);
    });
    if remove {
        actions.remove_object = Some(id);
    }
    if let Some(set) = state.scene.objects.get_mut(id) {
        if set.visible() != visible {
            set.set_visible(visible);
        }
        if set.color() != color {
            set.set_color(color);
        }
    }
}

fn params(ui: &mut Ui, state: &mut UiState<'_>, id: ObjectSetId) {
    let Some(channel) = state.scene.objects.get(id).map(ObjectSet::channel) else {
        return;
    };
    let voxel = state.scene.dataset.voxel_size().min_element();
    let data = state.scene.dataset.channels()[channel].intensity();
    let mut params = state
        .scene
        .objects
        .get(id)
        .map(ObjectSet::params)
        .expect("the object set exists");
    let mut changed = false;
    match &mut params {
        ObjectParams::Spots(spots) => {
            let mut diameter = spots.diameter();
            if ui
                .add(
                    egui::Slider::new(&mut diameter, voxel..=16.0 * voxel)
                        .logarithmic(true)
                        .text("diameter")
                        .suffix(" µm"),
                )
                .changed()
            {
                spots.set_diameter(diameter);
                changed = true;
            }
            let mut contrast = spots.threshold();
            if ui
                .add(egui::Slider::new(&mut contrast, 0.0..=1.0).text("contrast"))
                .changed()
            {
                spots.set_threshold(contrast);
                changed = true;
            }
        }
        ObjectParams::Surfaces(surfaces) => {
            let mut threshold = surfaces.threshold().clamp(data.min(), data.max());
            if ui
                .add(egui::Slider::new(&mut threshold, data.min()..=data.max()).text("threshold"))
                .changed()
            {
                surfaces.set_threshold(threshold);
                changed = true;
            }
            let mut min_volume = surfaces.min_volume();
            if ui
                .add(
                    egui::Slider::new(&mut min_volume, 0.0..=200.0)
                        .text("min volume")
                        .suffix(" µm³"),
                )
                .changed()
            {
                surfaces.set_min_volume(min_volume);
                changed = true;
            }
        }
    }
    if changed && let Some(set) = state.scene.objects.get_mut(id) {
        set.set_params(params);
    }
}

fn detect(
    ui: &mut Ui,
    state: &UiState<'_>,
    actions: &mut Actions,
    id: ObjectSetId,
    kind: ObjectKind,
    frames: u32,
) {
    let running = progress(state.analysis, id, Job::Detect);
    ui.horizontal(|ui| {
        if ui
            .add_enabled(running.is_none(), egui::Button::new("Detect"))
            .clicked()
        {
            actions.detect = Some(id);
        }
        if running.is_some() {
            if ui.button("Cancel").clicked() {
                actions.cancel_detect = Some(id);
            }
            ui.spinner();
        }
    });
    if let Some(running) = running {
        ui.label(
            RichText::new(caption(kind, Job::Detect, running, frames))
                .small()
                .weak(),
        );
    }
}

struct Summary {
    kind: ObjectKind,
    channel: String,
    current: Option<(usize, usize)>,
    mean: Option<f32>,
    detected: usize,
    frames: u32,
    time: u32,
}

fn summary_rows(ui: &mut Ui, summary: &Summary) {
    let Summary {
        kind,
        channel,
        current,
        mean,
        detected,
        frames,
        time,
    } = summary;
    match current {
        Some((count, level)) => {
            let noun = match kind {
                ObjectKind::Spots => "spots",
                ObjectKind::Surfaces => "objects",
            };
            ui.label(format!("{count} {noun} · level {}", level + 1));
            if let Some(mean) = mean {
                ui.label(
                    RichText::new(format!("mean contrast {mean:.2}"))
                        .small()
                        .weak(),
                );
            }
        }
        None if *detected == 0 => {
            let verb = match kind {
                ObjectKind::Spots => "detects intensity maxima in",
                ObjectKind::Surfaces => "extracts connected surfaces of",
            };
            ui.label(
                RichText::new(format!("{verb} {channel} in every frame"))
                    .small()
                    .weak(),
            );
            return;
        }
        None => {
            ui.label(
                RichText::new(format!("no objects on frame {} of {}", time + 1, frames))
                    .small()
                    .weak(),
            );
        }
    }
    if *frames > 1 && *detected > 0 {
        ui.label(
            RichText::new(format!("detected in {detected} frames"))
                .small()
                .weak(),
        );
    }
}

fn measure(ui: &mut Ui, state: &mut UiState<'_>, id: ObjectSetId, time: u32) {
    let selected = state.scene.selection().and_then(|selection| {
        (selection.set == id)
            .then_some(selection.item)
            .and_then(|item| match item {
                Item::Object(index) => Some(index),
                Item::Track(_) => None,
            })
    });
    let Some(object) = state.scene.objects.get(id) else {
        return;
    };
    let Some(frame) = object.frame(time) else {
        return;
    };
    let channel = object.channel();
    match frame {
        ObjectFrame::Spots(spots) => {
            let Some(index) = selected else {
                ui.label(RichText::new("click a spot to measure it").small().weak());
                return;
            };
            let Some(spot) = spots.spots().get(index).copied() else {
                return;
            };
            ui.separator();
            egui::Grid::new(("grana.spot", id.raw()))
                .num_columns(2)
                .spacing([12.0, 3.0])
                .show(ui, |ui| {
                    row(ui, "object", format!("#{}", index + 1));
                    let centre = spot.center();
                    row(
                        ui,
                        "position",
                        format!("{:.2} · {:.2} · {:.2} µm", centre[0], centre[1], centre[2]),
                    );
                    row(ui, "diameter", format!("{:.3} µm", spot.radius() * 2.0));
                    row(ui, "contrast", format!("{:.3}", spot.quality()));
                    let intensity = frame.intensities().object(index, channel);
                    row(
                        ui,
                        "intensity",
                        format!(
                            "{:.0} mean · {:.0} … {:.0} · σ {:.0}",
                            intensity.mean(),
                            intensity.minimum(),
                            intensity.maximum(),
                            intensity.deviation()
                        ),
                    );
                });
        }
        ObjectFrame::Surfaces(surfaces) => {
            let mut entries = surfaces
                .objects()
                .iter()
                .enumerate()
                .map(|(index, object)| (index, object.volume()))
                .collect::<Vec<_>>();
            entries.sort_by(|left, right| right.1.total_cmp(&left.1));
            let measured = selected.and_then(|index| surfaces.objects().get(index).copied());
            let intensity = measured.map(|_| {
                surfaces
                    .intensities()
                    .object(selected.unwrap_or(0), channel)
            });
            let selection = state.scene.selection();
            let mut clicked = None;
            egui::ScrollArea::vertical()
                .id_salt(("grana.surfaces", id.raw()))
                .max_height(150.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    for (index, volume) in &entries {
                        let selected = selection
                            == Some(Selection {
                                set: id,
                                item: Item::Object(*index),
                            });
                        if ui
                            .selectable_label(selected, format!("#{} · {volume:.1} µm³", index + 1))
                            .clicked()
                        {
                            clicked = Some(*index);
                        }
                    }
                });
            if let Some(index) = clicked {
                state.scene.select(Some(Selection {
                    set: id,
                    item: Item::Object(index),
                }));
            }
            let Some(object) = measured else {
                ui.label(
                    RichText::new("click an object to measure it")
                        .small()
                        .weak(),
                );
                return;
            };
            let Some(intensity) = intensity else {
                return;
            };
            let bounds = object.bounds();
            let size = bounds.size();
            let centroid = object.centroid();
            egui::Grid::new(("grana.surface", id.raw()))
                .num_columns(2)
                .spacing([12.0, 3.0])
                .show(ui, |ui| {
                    if let Some(index) = selected {
                        row(ui, "object", format!("#{}", index + 1));
                    }
                    row(ui, "volume", format!("{:.2} µm³", object.volume()));
                    row(ui, "area", format!("{:.2} µm²", object.area()));
                    row(ui, "sphericity", format!("{:.3}", object.sphericity()));
                    row(
                        ui,
                        "centroid",
                        format!(
                            "{:.2} · {:.2} · {:.2} µm",
                            centroid[0], centroid[1], centroid[2]
                        ),
                    );
                    row(
                        ui,
                        "extent",
                        format!("{:.2} × {:.2} × {:.2} µm", size[0], size[1], size[2]),
                    );
                    row(ui, "voxels", object.voxel_count().to_string());
                    row(
                        ui,
                        "intensity",
                        format!(
                            "{:.0} mean · {:.0} … {:.0} · σ {:.0}",
                            intensity.mean(),
                            intensity.minimum(),
                            intensity.maximum(),
                            intensity.deviation()
                        ),
                    );
                });
        }
    }
}
