use egui::{RichText, Ui};

use crate::analysis::Job;
use crate::objects::ObjectSetId;
use crate::ui::{Actions, UiState, caption, progress, row};
use grana_analysis::TrackParams;
use grana_object::{TrackSet, TrackStatistics};

pub fn build(ui: &mut Ui, state: &mut UiState<'_>, actions: &mut Actions, id: ObjectSetId) {
    let frame_count = state.scene.frame_count();
    if frame_count < 2 {
        return;
    }
    let selected = state.scene.selected_track(id);
    let Some(object) = state.scene.objects.get(id) else {
        return;
    };
    let kind = object.kind();
    let detected = object
        .frames()
        .map(|(_, frame)| frame.object_count())
        .sum::<usize>();
    let params = object.track_params();
    let min_thickness = object.min_thickness();
    let max_thickness = object.max_thickness();
    let view = object
        .tracks()
        .map(|layer| TrackView::new(layer.set(), selected, layer.visible(), layer.thickness()));
    let voxel = state.scene.dataset.voxel_size().min_element();
    let world = state.scene.dataset.world_size().max_element();

    ui.strong("Tracks");
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
    let updated = TrackParams::new(step, gap, division, range.max(step));
    if updated != params
        && let Some(object) = state.scene.objects.get_mut(id)
    {
        object.set_track_params(updated);
    }

    let tracking = progress(state.analysis, id, Job::Track);
    let detecting = progress(state.analysis, id, Job::Detect);
    ui.horizontal(|ui| {
        let enabled = tracking.is_none() && detecting.is_none() && detected > 0;
        if ui
            .add_enabled(enabled, egui::Button::new("Track"))
            .clicked()
        {
            actions.track = Some(id);
        }
        if tracking.is_some() {
            if ui.button("Cancel").clicked() {
                actions.cancel_track = Some(id);
            }
            ui.spinner();
        }
    });
    if let Some(tracking) = tracking {
        ui.label(
            RichText::new(caption(kind, Job::Track, tracking, frame_count))
                .small()
                .weak(),
        );
    }

    let Some(view) = view else {
        let hint = if detected == 0 {
            format!("detect {} before tracking them", kind.name().to_lowercase())
        } else {
            "build tracks to follow objects through time".to_string()
        };
        ui.label(RichText::new(hint).small().weak());
        return;
    };

    ui.label(format!(
        "{} tracks · {} divisions · {} nodes",
        view.count, view.divisions, view.nodes
    ));
    let mut visible = view.visible;
    if ui.checkbox(&mut visible, "visible").changed()
        && let Some(object) = state.scene.objects.get_mut(id)
    {
        object.set_track_visible(visible);
    }
    let mut thickness = view.thickness;
    if ui
        .add(
            egui::Slider::new(&mut thickness, min_thickness..=max_thickness)
                .logarithmic(true)
                .text("tube width")
                .suffix(" µm"),
        )
        .changed()
        && let Some(object) = state.scene.objects.get_mut(id)
    {
        object.set_track_thickness(thickness);
    }
    ui.horizontal(|ui| {
        if ui.button("Export CSV").clicked() {
            actions.export_tracks = Some(id);
        }
        if ui.button("Clear").clicked() {
            actions.clear_tracks = Some(id);
        }
    });

    let mut entries = view.entries;
    entries.sort_by(|left, right| right.2.total_cmp(&left.2));
    let mut clicked = None;
    egui::ScrollArea::vertical()
        .id_salt(("grana.tracks", id.raw()))
        .max_height(140.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            for (index, duration, length) in &entries {
                if ui
                    .selectable_label(
                        selected == Some(*index),
                        format!("#{} · {duration:.1} s · {length:.2} µm", index + 1),
                    )
                    .clicked()
                {
                    clicked = Some(*index);
                }
            }
        });
    if let Some(index) = clicked {
        state.scene.select_track(id, index);
    }

    let Some(selected) = view.selected else {
        ui.label(
            RichText::new("select a track to inspect its lineage")
                .small()
                .weak(),
        );
        return;
    };
    egui::Grid::new(("grana.track", id.raw()))
        .num_columns(2)
        .spacing([12.0, 3.0])
        .show(ui, |ui| {
            row(ui, "object", format!("#{}", selected.index + 1));
            row(
                ui,
                "parent",
                selected
                    .parent
                    .map_or_else(|| "root".to_string(), |parent| format!("#{}", parent + 1)),
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
                        .map(|child| format!("#{}", child + 1))
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

struct TrackView {
    count: usize,
    divisions: usize,
    nodes: usize,
    visible: bool,
    thickness: f32,
    entries: Vec<(usize, f64, f64)>,
    selected: Option<SelectedTrack>,
}

impl TrackView {
    fn new(set: &TrackSet, selected: Option<usize>, visible: bool, thickness: f32) -> Self {
        let interval = set.frame_interval();
        let entries = (0..set.len())
            .map(|index| {
                let statistics = set.statistics(index).expect("the track has statistics");
                (index, statistics.duration(), statistics.length())
            })
            .collect();
        let selected = selected.and_then(|index| {
            let track = set.track(index)?;
            Some(SelectedTrack {
                index,
                parent: track.parent().map(|parent| parent as usize),
                children: set.children(index).collect(),
                statistics: track.statistics(interval),
            })
        });
        Self {
            count: set.len(),
            divisions: set.divisions(),
            nodes: set.node_count(),
            visible,
            thickness,
            entries,
            selected,
        }
    }
}

struct SelectedTrack {
    index: usize,
    parent: Option<usize>,
    children: Vec<usize>,
    statistics: TrackStatistics,
}
