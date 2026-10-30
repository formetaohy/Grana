use crate::analysis::Kind;
use crate::scene::Scene;
use crate::ui::{Actions, UiState, caption, color_swatch, progress, row};
use egui::{RichText, Ui};
use grana_analysis::TrackParams;
use grana_object::{TrackSource, TrackStatistics};

pub fn build(ui: &mut Ui, state: &mut UiState<'_>, actions: &mut Actions, source: TrackSource) {
    let kind = Kind::track(source);
    let title = match source {
        TrackSource::Spots => "Spot tracks",
        TrackSource::Surfaces => "Surface tracks",
    };
    ui.strong(title);
    let tracking = progress(state.analysis, kind);
    let detected = match source {
        TrackSource::Spots => state
            .scene
            .spot_frames()
            .map(|(_, set)| set.len())
            .sum::<usize>(),
        TrackSource::Surfaces => state
            .scene
            .surface_frames()
            .map(|(_, set)| set.object_count())
            .sum::<usize>(),
    };
    let mut params = state.scene.tracks.params(source);
    let voxel = state.scene.tracks.voxel();
    let world = state.scene.dataset.world_size().max_element();
    let mut step = params.max_step();
    let mut gap = params.max_gap();
    let mut division = params.division();
    let mut range = params.division_range();
    let step_max = (0.25 * world).max(step * 4.0).max(16.0 * voxel);
    ui.add(
        egui::Slider::new(&mut step, voxel..=step_max)
            .logarithmic(true)
            .text("step")
            .suffix(" µm"),
    );
    ui.add(egui::Slider::new(&mut gap, 0..=10).text("gap"));
    ui.checkbox(&mut division, "division");
    if division {
        let minimum = step;
        let maximum = (4.0 * step).max(minimum);
        range = range.clamp(minimum, maximum);
        ui.add(
            egui::Slider::new(&mut range, minimum..=maximum)
                .text("division range")
                .suffix(" µm"),
        );
    }
    params = TrackParams::new(step, gap, division, range.max(step));
    state.scene.tracks.set_params(source, params);

    ui.horizontal(|ui| {
        let detection = match source {
            TrackSource::Spots => Kind::Spots,
            TrackSource::Surfaces => Kind::Surfaces,
        };
        let detection_running = progress(state.analysis, detection).is_some();
        let enabled = tracking.is_none() && detected > 0 && !detection_running;
        if ui
            .add_enabled(enabled, egui::Button::new("Track"))
            .clicked()
        {
            actions.track = Some(source);
        }
        if tracking.is_some() {
            if ui.button("Cancel").clicked() {
                actions.cancel_tracks = Some(source);
            }
            ui.spinner();
        }
        if let Some(progress) = tracking {
            ui.label(RichText::new(caption(state, progress)).small().weak());
        }
    });

    let Some(layer) = state.scene.tracks.layer(source) else {
        let hint = if detected == 0 {
            format!("detect {} before tracking them", source.name())
        } else {
            "build tracks to follow objects through time".to_string()
        };
        ui.label(RichText::new(hint).small().weak());
        return;
    };
    let set = layer.set();
    let count = set.len();
    let divisions = set.divisions();
    let nodes = set.node_count();
    let mut visible = layer.visible();
    let mut thickness = layer.thickness();
    let interval = set.frame_interval();
    let selected = state.scene.selected_track(source);
    let mut entries = (0..count)
        .map(|index| {
            let statistics = set.statistics(index).expect("the track has statistics");
            (index, statistics.duration(), statistics.length())
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| right.2.total_cmp(&left.2));
    let selected_view = selected.and_then(|index| {
        let track = set.track(index)?;
        Some(SelectedTrack {
            index,
            parent: track.parent().map(|parent| parent as usize + 1),
            children: set.children(index).map(|child| child + 1).collect(),
            statistics: track.statistics(interval),
        })
    });

    ui.label(format!(
        "{count} tracks · {divisions} divisions · {nodes} nodes"
    ));
    if ui.checkbox(&mut visible, "visible").changed() {
        state.scene.tracks.set_visible(source, visible);
    }
    let range = state.scene.tracks.min_thickness()..=state.scene.tracks.max_thickness();
    if ui
        .add(
            egui::Slider::new(&mut thickness, range)
                .logarithmic(true)
                .text("tube width")
                .suffix(" µm"),
        )
        .changed()
    {
        state.scene.tracks.set_thickness(source, thickness);
    }
    ui.horizontal(|ui| {
        if ui.button("Export CSV").clicked() {
            actions.export_tracks = Some(source);
        }
        if ui.button("Clear").clicked() {
            actions.clear_tracks = Some(source);
        }
    });
    let mut clicked = None;
    egui::ScrollArea::vertical()
        .id_salt(match source {
            TrackSource::Spots => "grana.spot.tracks",
            TrackSource::Surfaces => "grana.surface.tracks",
        })
        .max_height(140.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            for (index, duration, length) in &entries {
                ui.horizontal(|ui| {
                    color_swatch(ui, Scene::object_color(*index));
                    if ui
                        .selectable_label(
                            selected == Some(*index),
                            format!("#{} · {duration:.1} s · {length:.2} µm", index + 1),
                        )
                        .clicked()
                    {
                        clicked = Some(*index);
                    }
                });
            }
        });
    if let Some(index) = clicked {
        state.scene.select_track(source, index);
    }

    let Some(selected) = selected_view else {
        ui.label(
            RichText::new("select a track to inspect its lineage")
                .small()
                .weak(),
        );
        return;
    };
    let name = match source {
        TrackSource::Spots => "spot-track",
        TrackSource::Surfaces => "surface-track",
    };
    egui::Grid::new(name)
        .num_columns(2)
        .spacing([12.0, 3.0])
        .show(ui, |ui| {
            row(ui, "object", format!("#{}", selected.index + 1));
            row(
                ui,
                "parent",
                selected
                    .parent
                    .map_or_else(|| "root".to_string(), |parent| format!("#{parent}")),
            );
            row(
                ui,
                "daughters",
                if selected.children.is_empty() {
                    "none".to_string()
                } else {
                    selected
                        .children
                        .iter()
                        .map(|child| format!("#{child}"))
                        .collect::<Vec<_>>()
                        .join(" · ")
                },
            );
            row(ui, "frames", selected.statistics.frames().to_string());
            row(
                ui,
                "duration",
                format!("{:.2} s", selected.statistics.duration()),
            );
            row(
                ui,
                "length",
                format!("{:.2} µm", selected.statistics.length()),
            );
            row(
                ui,
                "displacement",
                format!("{:.2} µm", selected.statistics.displacement()),
            );
            row(
                ui,
                "speed",
                format!("{:.3} µm/s", selected.statistics.speed()),
            );
            row(
                ui,
                "straightness",
                format!("{:.3}", selected.statistics.straightness()),
            );
        });
}

struct SelectedTrack {
    index: usize,
    parent: Option<usize>,
    children: Vec<usize>,
    statistics: TrackStatistics,
}
