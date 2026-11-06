use glam::Vec2;
use grana_measure::Aggregate;
use grana_object::{TrackParams, TrackSet};
use grana_scene::{Job, ObjectSet, ObjectSetId, RelationField, TrackColor};
use grana_ui::{Align, Block, Color, Corners, Icon, Length, Rect, Role, TextStyle, TypeStyle, Ui};

use crate::panel::{Panel, card, card_title, empty_state, progress};
use crate::state::{State, Workspace};
use crate::widget;
use grana_session::Command;

const ROW: f32 = 13.0;
const AXIS: f32 = 18.0;
const SIDE: f32 = 10.0;
const INSPECTOR_WIDTH: f32 = 316.0;

pub fn entry(ui: &mut Ui, state: &mut State<'_>, id: ObjectSetId) {
    let scene = state.scene;
    let Some(object) = scene.objects.get(id) else {
        return;
    };
    if !object.kind().trackable() {
        return;
    }
    let counts = object
        .tracks()
        .map(|layer| (layer.set().len(), layer.set().divisions()));
    ui.row(Block::row().gaps(8.0).align(Align::Center), |ui| {
        match counts {
            Some((tracks, divisions)) => {
                widget::caption(ui, &format!("{tracks} tracks · {divisions} divisions"))
            }
            None => widget::caption(ui, "Not tracked yet"),
        }
        ui.space((ui.available().x - 86.0).max(0.0));
        if widget::button(ui, "tracks.open", "Lineage", Role::Control) {
            state.workspace.panel = Some(Panel::Tracks);
            state.workspace.track_set = Some(id);
        }
    });
}

pub fn build(ui: &mut Ui, state: &mut State<'_>, commands: &mut Vec<Command>) {
    let scene = state.scene;
    let sets = scene
        .objects
        .iter()
        .filter(|set| set.kind().trackable())
        .map(ObjectSet::id)
        .collect::<Vec<_>>();
    if sets.is_empty() {
        empty_state(
            ui,
            Icon::Channel,
            "No Trackable Objects",
            "Add spots or surfaces, detect them, then follow their trajectories through time.",
        );
        return;
    }
    if scene.frame_count() < 2 {
        empty_state(
            ui,
            Icon::Clock,
            "Single Time Frame",
            "Tracking follows objects across the frames of a time-series dataset.",
        );
        return;
    }
    let id = select(state, &sets);
    state.workspace.track_set = Some(id);
    let Some(object) = scene.objects.get(id) else {
        return;
    };
    let selected = scene.selected_track(id);
    header(ui, state, commands, &sets, id, object, selected);
    ui.space(8.0);
    let full = ui.allocate(ui.available());
    let gap = 12.0;
    let left_width = (full.width() - INSPECTOR_WIDTH - gap).max(260.0);
    let left_rect = Rect::from_min_size(full.min, Vec2::new(left_width, full.height()));
    let right_rect = Rect::from_min_size(
        Vec2::new(left_rect.max.x + gap, full.min.y),
        Vec2::new(INSPECTOR_WIDTH, full.height()),
    );
    ui.place(left_rect, |ui| {
        if let Some(set) = object.tracks().map(|layer| layer.set()) {
            let colors = (0..set.len())
                .map(|index| object.track_color(index))
                .collect::<Vec<_>>();
            lineage(
                ui,
                state.workspace,
                Tree {
                    set,
                    id,
                    colors: &colors,
                    selected,
                    time: scene.time(),
                    frames: scene.frame_count(),
                },
                left_rect.height(),
                commands,
            );
        } else {
            empty_state(
                ui,
                Icon::Channel,
                "No Trajectories Yet",
                "Detect objects in this set first, then press Track to link their lineage across frames.",
            );
        }
    });
    ui.stacked(right_rect, true, |ui| {
        ui.scroll(ui.id("tracks.inspector.scroll"), Length::Fill, |ui| {
            ui.column(Block::column().gaps(8.0), |ui| {
                if let Some(set) = object.tracks().map(|layer| layer.set()) {
                    summary(ui, set, selected);
                }
                parameters(ui, state, commands, id, object);
                appearance(ui, state, commands, id, object);
            });
        });
    });
}

fn select(state: &State<'_>, sets: &[ObjectSetId]) -> ObjectSetId {
    state
        .workspace
        .track_set
        .filter(|id| sets.contains(id))
        .or_else(|| {
            state
                .scene
                .selection()
                .map(|selection| selection.set)
                .filter(|id| sets.contains(id))
        })
        .unwrap_or(sets[0])
}

fn header(
    ui: &mut Ui,
    state: &mut State<'_>,
    commands: &mut Vec<Command>,
    sets: &[ObjectSetId],
    id: ObjectSetId,
    object: &ObjectSet,
    selected: Option<usize>,
) {
    let scene = state.scene;
    let names = sets
        .iter()
        .map(|set| {
            scene
                .objects
                .get(*set)
                .map_or_else(String::new, |set| set.name().to_string())
        })
        .collect::<Vec<_>>();
    let options = names.into_iter().enumerate().collect::<Vec<_>>();
    let chosen = sets.iter().position(|set| *set == id).unwrap_or(0);
    let tracking = progress(state.jobs, Job::Track(id));
    let detected = object.detected();
    let busy = state.io.loading.is_some();
    let divisions = object.tracks().map_or(0, |layer| layer.set().divisions());
    let tracks = object.tracks().map_or(0, |layer| layer.set().len());
    let time = scene.time();
    let cuts = object.tracks().map(|layer| layer.set()).and_then(|set| {
        let index = selected?;
        let track = set.track(index)?;
        let last = track.nodes().last()?;
        let node = track.nodes().iter().find(|node| node.time() == time)?;
        let division = set.children(index).next().is_some();
        (node.time() != last.time() || division).then_some((node.time(), node.object()))
    });
    let joins = object.tracks().map(|layer| layer.set()).and_then(|set| {
        let index = selected?;
        let track = set.track(index)?;
        let last = track.nodes().last()?;
        (set.children(index).next().is_none() && last.time() + 1 < scene.frame_count())
            .then_some((last.time(), last.object()))
    });
    let joining = state.workspace.join.is_some();
    let row = ui.allocate_full(30.0);
    let right_w = 340.0;
    let left_rect = Rect::from_min_max(row.min, Vec2::new(row.max.x - right_w - 8.0, row.max.y));
    let right_rect = Rect::from_min_max(Vec2::new(row.max.x - right_w, row.min.y), row.max);
    ui.place(left_rect, |ui| {
        ui.row(
            Block::row()
                .gaps(8.0)
                .align(Align::Center)
                .height(Length::Fill),
            |ui| {
                widget::field_label(ui, "Set");
                if let Some(index) =
                    grana_ui::popover::combo(ui, "tracks.set", 160.0, &options, Some(chosen))
                    && index != chosen
                {
                    state.workspace.join = None;
                    state.workspace.track_set = Some(sets[index]);
                }
                widget::caption(
                    ui,
                    &format!("{tracks} tracks · {divisions} divisions · {detected} frames"),
                );
                if joining {
                    widget::badge(ui, "Select target track to join", ui.theme().palette.accent);
                }
            },
        );
    });
    ui.place(right_rect, |ui| {
        ui.row(
            Block::row()
                .gaps(6.0)
                .align(Align::Center)
                .justify(Align::End)
                .width(Length::Fill)
                .height(Length::Fill),
            |ui| {
                if object.tracks().is_some() {
                    if button(ui, "cut", "Cut", Role::Control, cuts.is_some()) {
                        state.workspace.join = None;
                        if let Some((time, object)) = cuts {
                            commands.push(Command::CutTrack {
                                set: id,
                                time,
                                object,
                            });
                        }
                    }
                    if button(
                        ui,
                        "join",
                        if joining { "Cancel Join" } else { "Join" },
                        if joining { Role::Tinted } else { Role::Control },
                        joining || joins.is_some(),
                    ) {
                        state.workspace.join = if joining { None } else { joins };
                    }
                    if button(ui, "recompute", "Recompute", Role::Quiet, !busy) {
                        state.workspace.join = None;
                        commands.push(Command::Track(id));
                    }
                }
                if let Some(tracking) = tracking {
                    let fraction = if tracking.total > 0 {
                        tracking.done as f32 / tracking.total as f32
                    } else {
                        0.0
                    };
                    widget::progress(ui, fraction, 110.0);
                    widget::spinner(ui, 14.0);
                    if widget::button(ui, "track.cancel", "Cancel", Role::Quiet) {
                        commands.push(Command::CancelTrack(id));
                    }
                } else if button(ui, "track", "Track", Role::Prominent, detected > 0 && !busy) {
                    state.workspace.join = None;
                    commands.push(Command::Track(id));
                }
            },
        );
    });
}

fn parameters(
    ui: &mut Ui,
    state: &mut State<'_>,
    commands: &mut Vec<Command>,
    id: ObjectSetId,
    object: &ObjectSet,
) {
    let Some(style) = object.track_style() else {
        return;
    };
    let params = style.params();
    let voxel = state.scene.dataset.voxel_size().min_element();
    let world = state.scene.dataset.world_size().max_element();
    let visible = object.tracks().is_none_or(|layer| layer.visible());
    let thickness = object
        .tracks()
        .map_or_else(|| style.thickness(), |layer| layer.thickness());
    let min_thickness = object.min_thickness();
    let max_thickness = object.max_thickness();
    let mut step = params.max_step();
    let mut gap = params.max_gap() as f32;
    let mut division = params.division();
    let mut range = params.division_range();
    let mut width = thickness;
    let mut shown = visible;
    card(ui, 6.0, |ui| {
        card_title(ui, Icon::Sliders, "Parameters", None);
        let step_max = (0.25 * world).max(step * 4.0).max(16.0 * voxel);
        if let Some(value) = widget::slider_readout(
            ui,
            "step",
            Some("Max step"),
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
            Some("Max gap"),
            gap,
            0.0..=10.0,
            false,
            &format!("{gap:.0} f"),
        ) {
            gap = value;
        }
        if let Some(value) = widget::toggle_row(ui, "division", "Allow Division", division) {
            division = value;
        }
        let minimum = step;
        let maximum = (4.0 * step).max(minimum);
        if division {
            range = range.clamp(minimum, maximum);
            if let Some(value) = widget::slider_readout(
                ui,
                "division.range",
                Some("Div. range"),
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
        crate::panel::divider(ui);
        if let Some(value) = widget::toggle_row(ui, "visible", "Show Tubes", shown) {
            shown = value;
        }
        if let Some(value) = widget::slider_readout(
            ui,
            "width",
            Some("Tube width"),
            width,
            min_thickness..=max_thickness,
            true,
            &format!("{width:.3} µm"),
        ) {
            width = value;
        }
    });
    if shown != visible {
        commands.push(Command::SetTrackVisible {
            set: id,
            visible: shown,
        });
    }
    if (width - thickness).abs() > f32::EPSILON {
        commands.push(Command::SetTrackThickness {
            set: id,
            thickness: width,
        });
    }
}

fn appearance(
    ui: &mut Ui,
    state: &State<'_>,
    commands: &mut Vec<Command>,
    id: ObjectSetId,
    object: &ObjectSet,
) {
    if object.tracks().is_none() {
        return;
    }
    let mut metrics = object
        .colorable_track_measurements()
        .into_iter()
        .map(|measurement| {
            (
                TrackColor::Measurement(measurement),
                measurement.label().to_string(),
            )
        })
        .collect::<Vec<_>>();
    for relation in object.relations() {
        let name = state
            .scene
            .objects
            .get(relation.target())
            .map_or_else(|| "Target".to_string(), |target| target.name().to_string());
        let shift = match relation.shift() {
            0 => String::new(),
            shift => format!(" t{shift:+}"),
        };
        for field in RelationField::CONTINUOUS {
            for aggregate in Aggregate::ALL {
                metrics.push((
                    TrackColor::Relation {
                        target: relation.target(),
                        field,
                        aggregate,
                        shift: relation.shift(),
                    },
                    format!("{name} {}{shift} · {}", field.label(), aggregate.name()),
                ));
            }
        }
    }
    if metrics.is_empty() {
        return;
    }
    card(ui, 6.0, |ui| {
        card_title(ui, Icon::Sparkles, "Tube Coloring", None);
        let extent = object.track_extent();
        let mut coloring = object.track_coloring().clone();
        if crate::coloring::edit(
            ui,
            &format!("tracks.{}.coloring", id.raw()),
            &mut coloring,
            &metrics,
            extent,
            |metric| (metric.unit(), metric.decimals()),
        ) {
            commands.push(Command::SetTrackColoring { set: id, coloring });
        }
    });
}

struct Tree<'a> {
    set: &'a TrackSet,
    id: ObjectSetId,
    colors: &'a [[f32; 3]],
    selected: Option<usize>,
    time: u32,
    frames: u32,
}

fn lineage(
    ui: &mut Ui,
    workspace: &mut Workspace,
    tree: Tree<'_>,
    height: f32,
    commands: &mut Vec<Command>,
) {
    let Tree {
        set,
        id,
        colors,
        selected,
        time,
        frames,
    } = tree;
    let radius = ui.theme().metrics.card_radius;
    let accent = ui.theme().palette.accent;
    let tertiary = ui.theme().palette.text_tertiary;
    let rect = ui.allocate(Vec2::new(ui.available().x, height));
    let inner = Rect::from_min_max(
        rect.min + Vec2::new(SIDE, AXIS + 4.0),
        rect.max - Vec2::new(SIDE + 6.0, 6.0),
    );
    let order = lanes(set);
    let mut lane_of = vec![0usize; set.len()];
    for (lane, index) in order.iter().enumerate() {
        lane_of[*index] = lane;
    }
    let offset_key = ui.id("tracks.tree.offset");
    let content = order.len() as f32 * ROW;
    let limit = (content - inner.height()).max(0.0);
    let mut offset = ui.value_of(offset_key).unwrap_or(0.0).clamp(0.0, limit);
    let response = ui.interact(ui.id("tracks.tree"), rect);
    if response.hovered {
        let wheel = ui.input().wheel().y;
        if wheel != 0.0 {
            offset = (offset - wheel * 22.0).clamp(0.0, limit);
            ui.store_value(offset_key, offset);
        }
    }
    let pointer = ui.pointer_in(inner).filter(|_| response.hovered);
    let hovered = pointer.and_then(|point| {
        let lane = ((point.y - inner.min.y + offset) / ROW).floor();
        (lane >= 0.0)
            .then_some(lane as usize)
            .filter(|lane| *lane < order.len())
    });
    let span = frames.max(2) - 1;
    let x = |time: u32| inner.min.x + (time as f32 / span as f32) * inner.width();
    let lane_y = |lane: usize| inner.min.y + lane as f32 * ROW + ROW * 0.5 - offset;
    let source = workspace.join;
    let palette = ui.theme().palette;
    {
        let mut painter = ui.painter();
        painter.outlined(
            rect,
            Corners::all(radius),
            grana_ui::Fill::Solid(palette.card),
            grana_ui::Border {
                width: 1.0,
                color: palette.material_border,
            },
            grana_ui::Shadow::NONE,
        );
        let clip = painter.clip();
        painter.push_clip(inner, Corners::ZERO);
        for part in 1..4 {
            let grid = x(span * part / 4);
            painter.segment(
                Vec2::new(grid, inner.min.y),
                Vec2::new(grid, inner.max.y),
                0.5,
                palette.separator,
            );
        }
        painter.segment(
            Vec2::new(x(time), inner.min.y),
            Vec2::new(x(time), inner.max.y),
            1.0,
            accent.with_alpha(0.50),
        );
        if let Some(lane) = hovered {
            painter.rect(
                Rect::from_min_size(
                    Vec2::new(inner.min.x, lane_y(lane) - ROW * 0.5),
                    Vec2::new(inner.width(), ROW),
                ),
                Corners::all(3.0),
                Color::white_alpha(0.05),
            );
        }
        for (lane, index) in order.iter().enumerate() {
            let y = lane_y(lane);
            if y < inner.min.y - ROW || y > inner.max.y + ROW {
                continue;
            }
            let Some(track) = set.track(*index) else {
                continue;
            };
            let start = x(track.start());
            let end = x(track.end());
            let chosen = selected == Some(*index);
            let tint = if chosen {
                accent
            } else {
                Color::from_rgb_f32(colors.get(*index).copied().unwrap_or([0.6, 0.6, 0.6]))
            };
            if let Some(parent) = track.parent() {
                let parent = parent as usize;
                if let Some(track) = set.track(parent) {
                    painter.segment(
                        Vec2::new(x(track.end()), lane_y(lane_of[parent])),
                        Vec2::new(start, y),
                        1.0,
                        tint.with_alpha(0.35),
                    );
                }
            }
            painter.segment(
                Vec2::new(start, y),
                Vec2::new(end.max(start + 1.0), y),
                if chosen { 2.4 } else { 1.5 },
                tint,
            );
            let nodes = track.nodes();
            let spacing = if nodes.len() > 1 {
                (end - start) / (nodes.len() - 1) as f32
            } else {
                0.0
            };
            if spacing >= 2.6 {
                for node in nodes {
                    painter.rect(
                        Rect::from_center_size(Vec2::new(x(node.time()), y), Vec2::splat(2.4)),
                        Corners::capsule(),
                        tint.with_alpha(0.88),
                    );
                }
            } else if let (Some(first), Some(last)) = (nodes.first(), nodes.last()) {
                for node in [first, last] {
                    painter.rect(
                        Rect::from_center_size(Vec2::new(x(node.time()), y), Vec2::splat(2.8)),
                        Corners::capsule(),
                        tint,
                    );
                }
            }
        }
        if let Some(source) = source
            && let Some(track) = set.track_at(source.0, source.1)
        {
            let y = lane_y(lane_of[track]);
            painter.ring(
                Vec2::new(x(source.0), y),
                4.5,
                1.4,
                0.0,
                std::f32::consts::TAU,
                accent,
            );
            for (lane, index) in order.iter().enumerate() {
                let Some(track) = set.track(*index) else {
                    continue;
                };
                let Some(first) = track.nodes().first() else {
                    continue;
                };
                if set.can_join(source, (first.time(), first.object())) {
                    painter.rect(
                        Rect::from_center_size(
                            Vec2::new(x(first.time()), lane_y(lane)),
                            Vec2::splat(4.0),
                        ),
                        Corners::capsule(),
                        accent,
                    );
                }
            }
        }
        painter.pop_clip(clip);
        if limit > 0.0 {
            let bar = Rect::from_min_size(
                Vec2::new(rect.max.x - 5.0, inner.min.y),
                Vec2::new(2.5, inner.height()),
            );
            let thumb = (inner.height() * inner.height() / content).max(24.0);
            let travel = (bar.height() - thumb).max(0.0);
            painter.rect(bar, Corners::capsule(), Color::white_alpha(0.05));
            painter.rect(
                Rect::from_min_size(
                    Vec2::new(bar.min.x, bar.min.y + travel * offset / limit),
                    Vec2::new(2.5, thumb),
                ),
                Corners::capsule(),
                Color::white_alpha(0.24),
            );
        }
        let label = TextStyle::new(TypeStyle::new(9.5, 400.0, 12.0), tertiary);
        for part in 0..4 {
            let tick = span * part / 3;
            let centre = Vec2::new(x(tick), inner.min.y - AXIS * 0.5);
            let rect = if part == 3 {
                Rect::from_min_max(
                    centre - Vec2::new(64.0, AXIS * 0.5),
                    centre + Vec2::new(0.0, AXIS * 0.5),
                )
            } else {
                Rect::from_center_size(centre, Vec2::new(64.0, AXIS))
            };
            painter.text_in(
                rect,
                &format!("F{}", tick + 1),
                if part == 3 {
                    label.aligned(Align::End)
                } else {
                    label.aligned(Align::Center)
                },
                Align::Center,
            );
        }
        if let (Some(lane), Some(point)) = (hovered, pointer)
            && source.is_none()
        {
            painter.text_in(
                Rect::from_min_size(
                    Vec2::new(point.x + 10.0, lane_y(lane) - ROW * 0.5),
                    Vec2::new(64.0, ROW),
                ),
                &format!("#{}", order[lane] + 1),
                TextStyle::new(TypeStyle::new(9.5, 500.0, 12.0), tertiary),
                Align::Center,
            );
        }
    }
    if let Some(lane) = hovered
        && response.clicked
        && let Some(point) = pointer
    {
        let index = order[lane];
        if let Some(source) = source {
            if let Some(target) = set
                .track(index)
                .and_then(|track| track.nodes().first())
                .map(|node| (node.time(), node.object()))
                && set.can_join(source, target)
            {
                commands.push(Command::JoinTracks {
                    set: id,
                    source,
                    target,
                });
            }
            workspace.join = None;
        } else {
            commands.push(Command::SelectTrack {
                set: id,
                index: Some(index),
            });
            let target = ((point.x - inner.min.x) / inner.width() * span as f32)
                .clamp(0.0, span as f32)
                .round() as u32;
            if let Some(time) = set.track(index).and_then(|track| {
                track
                    .nodes()
                    .iter()
                    .min_by_key(|node| node.time().abs_diff(target))
                    .map(|node| node.time())
            }) {
                commands.push(Command::SetTime(time));
            }
        }
    }
}

fn summary(ui: &mut Ui, set: &TrackSet, selected: Option<usize>) {
    let Some(index) = selected else {
        card(ui, 4.0, |ui| {
            card_title(ui, Icon::Target, "Selected Track", Some("None"));
            widget::caption(ui, "Click a track in the lineage to inspect metrics.");
        });
        return;
    };
    let Some(track) = set.track(index) else {
        return;
    };
    let statistics = track.statistics(set.frame_interval());
    let parent = track
        .parent()
        .map_or_else(|| "Root".to_string(), |parent| format!("#{}", parent + 1));
    let children = set.children(index).collect::<Vec<_>>();
    let daughters = if children.is_empty() {
        "None".to_string()
    } else {
        children
            .iter()
            .map(|child| format!("#{}", child + 1))
            .collect::<Vec<_>>()
            .join(" · ")
    };
    card(ui, 3.0, |ui| {
        card_title(
            ui,
            Icon::Target,
            &format!("Track #{}", index + 1),
            Some(&format!("{} frames", statistics.frames())),
        );
        widget::labelled(ui, "Parent", &parent);
        widget::labelled(ui, "Daughters", &daughters);
        widget::labelled(ui, "Duration", &format!("{:.2} s", statistics.duration()));
        widget::labelled(ui, "Length", &format!("{:.2} µm", statistics.length()));
        widget::labelled(
            ui,
            "Displacement",
            &format!("{:.2} µm", statistics.displacement()),
        );
        widget::labelled(ui, "Speed", &format!("{:.3} µm/s", statistics.speed()));
        widget::labelled(
            ui,
            "Straightness",
            &format!("{:.3}", statistics.straightness()),
        );
    });
}

fn lanes(set: &TrackSet) -> Vec<usize> {
    let mut children = vec![Vec::new(); set.len()];
    let mut roots = Vec::new();
    for index in 0..set.len() {
        match set.track(index).and_then(|track| track.parent()) {
            Some(parent) => children[parent as usize].push(index),
            None => roots.push(index),
        }
    }
    let mut order = Vec::with_capacity(set.len());
    let mut stack = roots.iter().rev().copied().collect::<Vec<_>>();
    while let Some(index) = stack.pop() {
        order.push(index);
        for child in children[index].iter().rev() {
            stack.push(*child);
        }
    }
    order
}

fn button(ui: &mut Ui, id: &str, label: &str, role: Role, enabled: bool) -> bool {
    if enabled {
        return widget::button(ui, id, label, role);
    }
    let font = ui.theme().font(grana_ui::Level::Body);
    let measured = ui.measure_text(label, TextStyle::new(font, Color::WHITE));
    let rect = ui.allocate(Vec2::new(
        measured.x + 22.0,
        ui.theme().metrics.control_height,
    ));
    let look = ui.surface_disabled(role);
    let mut painter = ui.painter();
    painter.outlined(rect, look.corners, look.fill, look.border, look.shadow);
    painter.text_in(
        rect,
        label,
        TextStyle::new(font, look.text).aligned(Align::Center),
        Align::Center,
    );
    false
}
