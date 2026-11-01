use grana_analysis::TrackParams;
use grana_object::{TrackSet, TrackStatistics};
use grana_scene::{Job, ObjectSetId};
use grana_ui::{Align, Axis, Block, Length, Role, Ui};

use crate::panel::{caption, progress};
use crate::state::State;
use crate::widget;
use grana_session::Command;

pub fn build(ui: &mut Ui, state: &mut State<'_>, commands: &mut Vec<Command>, id: ObjectSetId) {
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
    let params = object
        .track_style()
        .expect("the track view belongs to a trackable object")
        .params();
    let min_thickness = object.min_thickness();
    let max_thickness = object.max_thickness();
    let view = object
        .tracks()
        .map(|layer| TrackView::new(layer.set(), selected, layer.visible(), layer.thickness()));
    let voxel = state.scene.dataset.voxel_size().min_element();
    let world = state.scene.dataset.world_size().max_element();
    let open = ui.disclosure_open("tracks", true);
    let next = widget::disclosure(ui, "tracks", "Tracks", open, |ui| {
        let mut step = params.max_step();
        let mut gap = params.max_gap() as f32;
        let mut division = params.division();
        let mut range = params.division_range();
        let step_max = (0.25 * world).max(step * 4.0).max(16.0 * voxel);
        if let Some(value) = widget::slider_readout(
            ui,
            "step",
            Some("step"),
            step,
            voxel..=step_max,
            true,
            &format!("{step:.3} µm"),
        ) {
            step = value;
        }
        if let Some(value) = widget::slider_readout(
            ui,
            "gap",
            Some("gap"),
            gap,
            0.0..=10.0,
            false,
            &format!("{gap:.0}"),
        ) {
            gap = value;
        }
        if let Some(value) = widget::toggle_row(ui, "division", "division", division) {
            division = value;
        }
        if division {
            let minimum = step;
            let maximum = (4.0 * step).max(minimum);
            range = range.clamp(minimum, maximum);
            if let Some(value) = widget::slider_readout(
                ui,
                "division.range",
                Some("division range"),
                range,
                minimum..=maximum,
                false,
                &format!("{range:.2} µm"),
            ) {
                range = value;
            }
        }
        let updated = TrackParams::new(step, gap.round() as u32, division, range.max(step));
        if updated != params {
            commands.push(Command::SetTrackParams {
                set: id,
                params: updated,
            });
        }
        let tracking = progress(state.jobs, Job::Track(id));
        let detecting = progress(state.jobs, Job::Detect(id));
        ui.row(Block::row().gaps(8.0).align(Align::Center), |ui| {
            let enabled = tracking.is_none() && detecting.is_none() && detected > 0;
            if enabled && widget::button(ui, "track", "Track", Role::Prominent) {
                commands.push(Command::Track(id));
            }
            if tracking.is_some() {
                if widget::button(ui, "cancel", "Cancel", Role::Quiet) {
                    commands.push(Command::CancelTrack(id));
                }
                widget::spinner(ui, 14.0);
            }
        });
        if let Some(tracking) = tracking {
            caption(
                ui,
                &crate::panel::progress_caption(kind, Job::Track(id), tracking, frame_count),
            );
        }
        let Some(view) = view else {
            let hint = if detected == 0 {
                format!("detect {} before tracking them", kind.name().to_lowercase())
            } else {
                "build tracks to follow objects through time".to_string()
            };
            widget::weak(ui, &hint);
            return;
        };
        widget::caption(
            ui,
            &format!(
                "{} tracks · {} divisions · {} nodes",
                view.count, view.divisions, view.nodes
            ),
        );
        if let Some(value) = widget::toggle_row(ui, "visible", "visible", view.visible) {
            commands.push(Command::SetTrackVisible {
                set: id,
                visible: value,
            });
        }
        if let Some(value) = widget::slider_readout(
            ui,
            "width",
            Some("tube width"),
            view.thickness,
            min_thickness..=max_thickness,
            true,
            &format!("{:.3} µm", view.thickness),
        ) {
            commands.push(Command::SetTrackThickness {
                set: id,
                thickness: value,
            });
        }
        let mut entries = view.entries.clone();
        entries.sort_by(|left, right| right.2.total_cmp(&left.2));
        let mut clicked = None;
        let mut scroll = ui.id("track.list");
        let _ = scroll;
        scroll = ui.id("track.list");
        ui.scroll_axis(scroll, Axis::Column, Length::Fixed(132.0), None, |ui| {
            for (index, duration, length) in &entries {
                let label = format!("#{} · {duration:.1} s · {length:.2} µm", index + 1);
                if crate::object_panel::row_button(
                    ui,
                    &format!("track.{index}"),
                    &label,
                    selected == Some(*index),
                ) {
                    clicked = Some(*index);
                }
            }
        });
        if let Some(index) = clicked {
            commands.push(Command::SelectTrack {
                set: id,
                index: Some(index),
            });
        }
        let Some(selected) = view.selected else {
            widget::weak(ui, "select a track to inspect its lineage");
            return;
        };
        widget::labelled(ui, "object", &format!("#{}", selected.index + 1));
        widget::labelled(
            ui,
            "parent",
            &selected
                .parent
                .map_or_else(|| "root".to_string(), |parent| format!("#{}", parent + 1)),
        );
        widget::labelled(
            ui,
            "daughters",
            &if selected.children.is_empty() {
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
        widget::labelled(ui, "frames", &selected.statistics.frames().to_string());
        widget::labelled(
            ui,
            "duration",
            &format!("{:.2} s", selected.statistics.duration()),
        );
        widget::labelled(
            ui,
            "length",
            &format!("{:.2} µm", selected.statistics.length()),
        );
        widget::labelled(
            ui,
            "displacement",
            &format!("{:.2} µm", selected.statistics.displacement()),
        );
        widget::labelled(
            ui,
            "speed",
            &format!("{:.3} µm/s", selected.statistics.speed()),
        );
        widget::labelled(
            ui,
            "straightness",
            &format!("{:.3}", selected.statistics.straightness()),
        );
    });
    ui.store_open("tracks", next);
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
