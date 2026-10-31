use egui::{RichText, Ui};
use grana_scene::{
    Item, Job, ObjectFrame, ObjectKind, ObjectSet, ObjectSetId, ParamBound, ParamField, Selection,
};
use grana_volume::ChannelInfo;

use crate::action::Action;
use crate::panel::{caption, progress, row};
use crate::state::State;

pub fn build(ui: &mut Ui, state: &mut State<'_>, actions: &mut Vec<Action>) {
    ui.strong("Objects");
    let channel = state.scene.selected;
    let channel_name = state.scene.channel(channel).name().to_string();
    let loading = state.loading.is_some();
    ui.horizontal_wrapped(|ui| {
        for kind in [
            ObjectKind::Spots,
            ObjectKind::Surfaces,
            ObjectKind::Filaments,
        ] {
            if ui
                .add_enabled(!loading, egui::Button::new(format!("+ {}", kind.name())))
                .clicked()
            {
                actions.push(Action::AddObject(kind));
            }
        }
    });
    ui.label(
        RichText::new(format!("new objects read {channel_name}"))
            .small()
            .weak(),
    );
    if state.scene.objects.is_empty() {
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

fn object(ui: &mut Ui, state: &mut State<'_>, actions: &mut Vec<Action>, id: ObjectSetId) {
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
                actions.push(Action::ClearObject(id));
            }
        });
        measure(ui, state, id, time);
        if kind.trackable() {
            crate::track_panel::build(ui, state, actions, id);
        }
    });
    if remove {
        actions.push(Action::RemoveObject(id));
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

fn params(ui: &mut Ui, state: &mut State<'_>, id: ObjectSetId) {
    let Some(set) = state.scene.objects.get(id) else {
        return;
    };
    let kind = set.kind();
    let channel = set.channel();
    let info = &state.scene.dataset.channels()[channel];
    let voxel = state.scene.dataset.voxel_size().min_element();
    let world = state.scene.dataset.world_size().max_element();
    let mut params = set.params();
    let mut changed = false;
    for (index, field) in kind.fields().iter().enumerate() {
        let (minimum, maximum) = bounds(field, info, voxel, world);
        let mut value = params.value(index).clamp(minimum, maximum);
        let mut slider = egui::Slider::new(&mut value, minimum..=maximum)
            .text(field.label)
            .logarithmic(field.logarithmic);
        if !field.suffix.is_empty() {
            slider = slider.suffix(field.suffix);
        }
        if ui.add(slider).changed() {
            params.set_value(index, value);
            changed = true;
        }
    }
    if changed && let Some(set) = state.scene.objects.get_mut(id) {
        set.set_params(params);
    }
}

fn bounds(field: &ParamField, channel: &ChannelInfo, voxel: f32, world: f32) -> (f32, f32) {
    match field.bound {
        ParamBound::Voxel(factor) => (voxel, factor * voxel),
        ParamBound::Unit => (0.0, 1.0),
        ParamBound::Intensity => (channel.intensity().min(), channel.intensity().max()),
        ParamBound::Volume => (0.0, 200.0),
        ParamBound::Length(fraction) => (voxel, fraction * world),
    }
}

fn detect(
    ui: &mut Ui,
    state: &State<'_>,
    actions: &mut Vec<Action>,
    id: ObjectSetId,
    kind: ObjectKind,
    frames: u32,
) {
    let running = progress(state.analysis, Job::Detect(id));
    ui.horizontal(|ui| {
        if ui
            .add_enabled(running.is_none(), egui::Button::new("Detect"))
            .clicked()
        {
            actions.push(Action::Detect(id));
        }
        if running.is_some() {
            if ui.button("Cancel").clicked() {
                actions.push(Action::CancelDetect(id));
            }
            ui.spinner();
        }
    });
    if let Some(running) = running {
        ui.label(
            RichText::new(caption(kind, Job::Detect(id), running, frames))
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
            ui.label(format!(
                "{count} {} · level {}",
                kind.count_noun(),
                level + 1
            ));
            if let Some(mean) = mean {
                ui.label(
                    RichText::new(format!("mean contrast {mean:.2}"))
                        .small()
                        .weak(),
                );
            }
        }
        None if *detected == 0 => {
            ui.label(
                RichText::new(format!("{} {channel} in every frame", kind.description()))
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

fn measure(ui: &mut Ui, state: &mut State<'_>, id: ObjectSetId, time: u32) {
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
            object_list(ui, state, id, &entries, "grana.surfaces", |volume| {
                format!("{volume:.1} µm³")
            });
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
                    let centroid = object.centroid();
                    row(
                        ui,
                        "centroid",
                        format!(
                            "{:.2} · {:.2} · {:.2} µm",
                            centroid[0], centroid[1], centroid[2]
                        ),
                    );
                    let size = object.bounds().size();
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
        ObjectFrame::Filaments(filaments) => {
            let mut entries = filaments
                .objects()
                .iter()
                .enumerate()
                .map(|(index, object)| (index, object.length()))
                .collect::<Vec<_>>();
            entries.sort_by(|left, right| right.1.total_cmp(&left.1));
            let measured = selected.and_then(|index| filaments.object(index));
            let intensity = measured.map(|_| {
                filaments
                    .intensities()
                    .object(selected.unwrap_or(0), channel)
            });
            object_list(ui, state, id, &entries, "grana.filaments", |length| {
                format!("{length:.1} µm")
            });
            let Some(object) = measured else {
                ui.label(
                    RichText::new("click a filament to measure it")
                        .small()
                        .weak(),
                );
                return;
            };
            let Some(intensity) = intensity else {
                return;
            };
            egui::Grid::new(("grana.filament", id.raw()))
                .num_columns(2)
                .spacing([12.0, 3.0])
                .show(ui, |ui| {
                    if let Some(index) = selected {
                        row(ui, "object", format!("#{}", index + 1));
                    }
                    row(ui, "length", format!("{:.2} µm", object.length()));
                    row(ui, "diameter", format!("{:.2} µm", object.diameter()));
                    row(ui, "volume", format!("{:.2} µm³", object.volume()));
                    row(ui, "branches", object.branches().to_string());
                    row(ui, "terminals", object.terminals().to_string());
                    row(ui, "junctions", object.junctions().to_string());
                    row(ui, "nodes", object.nodes().to_string());
                    let centroid = object.centroid();
                    row(
                        ui,
                        "centroid",
                        format!(
                            "{:.2} · {:.2} · {:.2} µm",
                            centroid[0], centroid[1], centroid[2]
                        ),
                    );
                    let size = object.bounds().size();
                    row(
                        ui,
                        "extent",
                        format!("{:.2} × {:.2} × {:.2} µm", size[0], size[1], size[2]),
                    );
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

fn object_list(
    ui: &mut Ui,
    state: &mut State<'_>,
    id: ObjectSetId,
    entries: &[(usize, f64)],
    salt: &str,
    value: impl Fn(f64) -> String,
) {
    let selection = state.scene.selection();
    let mut clicked = None;
    egui::ScrollArea::vertical()
        .id_salt((salt, id.raw()))
        .max_height(150.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            for (index, amount) in entries {
                let selected = selection
                    == Some(Selection {
                        set: id,
                        item: Item::Object(*index),
                    });
                if ui
                    .selectable_label(selected, format!("#{} · {}", index + 1, value(*amount)))
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
}
