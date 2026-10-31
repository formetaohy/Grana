use glam::Vec2;
use grana_scene::{
    Item, Job, ObjectFrame, ObjectKind, ObjectSet, ObjectSetId, ParamBound, ParamField, Selection,
};
use grana_ui::{Align, Block, Color, Corners, Edges, Length, Rect, Role, TextStyle, Ui};

use crate::action::Action;
use crate::panel::{caption, progress};
use crate::state::State;
use crate::widget;

pub fn build(ui: &mut Ui, state: &mut State<'_>, actions: &mut Vec<Action>) {
    let open = ui.disclosure_open("objects", true);
    let next = widget::disclosure(ui, "objects", "Objects", open, |ui| {
        let channel = state.scene.selected;
        let channel_name = state.scene.channel(channel).name().to_string();
        let loading = state.loading.is_some();
        ui.row(Block::row().gaps(6.0), |ui| {
            for kind in [
                ObjectKind::Spots,
                ObjectKind::Surfaces,
                ObjectKind::Filaments,
            ] {
                let label = format!("+ {}", kind.name());
                if !loading
                    && widget::button(
                        ui,
                        &format!("add.{}.{}", kind.name(), channel),
                        &label,
                        Role::Quiet,
                    )
                {
                    actions.push(Action::AddObject(kind));
                }
            }
        });
        widget::weak(ui, &format!("new objects read {channel_name}"));
        if state.scene.objects.is_empty() {
            widget::weak(ui, "add an object, then detect and track it");
            return;
        }
        let ids = state
            .scene
            .objects
            .iter()
            .map(ObjectSet::id)
            .collect::<Vec<_>>();
        for id in ids {
            object(ui, state, actions, id);
        }
    });
    ui.store_open("objects", next);
}

fn object(ui: &mut Ui, state: &mut State<'_>, actions: &mut Vec<Action>, id: ObjectSetId) {
    let Some(set) = state.scene.objects.get(id) else {
        return;
    };
    let kind = set.kind();
    let name = set.name().to_string();
    let channel = set.channel();
    let channel_name = state.scene.dataset.channels()[channel].name().to_string();
    let color = set.color();
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
    let coloring = set.object_coloring().clone();
    let palette = coloring.palette();
    let reversed = coloring.reversed();
    let colored = coloring.metric().is_some();
    let open_id = format!("object.{}.open", id.raw());
    let expanded = ui.disclosure_open(&open_id, true);
    let look = state_card(ui);
    ui.push_id(&format!("object.{}", id.raw()));
    ui.column(
        Block::column()
            .padding(Edges::all(11.0))
            .gaps(8.0)
            .background(look),
        |ui| {
            let header = ui.allocate_full(24.0);
            let response = ui.interact(
                ui.id("header"),
                Rect::from_min_max(header.min, Vec2::new(header.max.x - 62.0, header.max.y)),
            );
            let arrow = ui.animate(
                ui.id("arrow"),
                if expanded { 1.0 } else { 0.0 },
                grana_ui::Motion::snappy(),
            );
            let chevron = ui.theme().palette.text_tertiary;
            let title = ui.theme().font(grana_ui::Level::Headline);
            let title_color = ui.theme().palette.text_primary;
            {
                let mut painter = ui.painter();
                if colored {
                    let samples = 16;
                    let width = 16.0;
                    let height = 12.0;
                    let rect = Rect::from_center_size(
                        Vec2::new(header.min.x + 9.0, header.center().y),
                        Vec2::new(width, height),
                    );
                    let step = width / samples as f32;
                    for index in 0..samples {
                        let position = index as f32 / (samples - 1) as f32;
                        let position = if reversed { 1.0 - position } else { position };
                        let [red, green, blue] = palette.sample(position);
                        painter.rect(
                            Rect::from_min_size(
                                Vec2::new(rect.min.x + index as f32 * step, rect.min.y),
                                Vec2::new(step + 0.5, height),
                            ),
                            Corners::all(1.0),
                            Color::rgb(red, green, blue),
                        );
                    }
                } else {
                    painter.rect(
                        Rect::from_center_size(
                            Vec2::new(header.min.x + 8.0, header.center().y),
                            Vec2::new(11.0, 11.0),
                        ),
                        Corners::all(3.0),
                        Color::from_rgb_f32(color),
                    );
                }
                let angle = std::f32::consts::FRAC_PI_2 * (0.5 + 0.5 * arrow);
                let centre = Vec2::new(header.min.x + 30.0, header.center().y);
                let tip = centre + Vec2::new(angle.cos(), angle.sin()) * 2.2;
                painter.segment(tip + Vec2::new(-2.0, -1.3), tip, 1.4, chevron);
                painter.segment(tip, tip + Vec2::new(2.0, -1.3), 1.4, chevron);
                painter.text_in(
                    Rect::from_min_max(
                        Vec2::new(header.min.x + 38.0, header.min.y),
                        Vec2::new(header.max.x - 62.0, header.max.y),
                    ),
                    &name,
                    TextStyle::new(title, title_color),
                    Align::Center,
                );
            }
            if response.clicked {
                ui.store_open(&open_id, !expanded);
            }
            ui.place(
                Rect::from_min_size(
                    Vec2::new(header.max.x - 58.0, header.min.y),
                    Vec2::new(34.0, header.height()),
                ),
                |ui| {
                    if let Some(value) = widget::toggle(ui, "visible", visible) {
                        visible = value;
                    }
                },
            );
            ui.place(
                Rect::from_min_size(
                    Vec2::new(header.max.x - 22.0, header.min.y),
                    Vec2::new(22.0, header.height()),
                ),
                |ui| {
                    if widget::icon_button(ui, "close", "\u{00D7}", 22.0, Role::Quiet) {
                        actions.push(Action::RemoveObject(id));
                    }
                },
            );
            if expanded {
                widget::weak(ui, &channel_name);
                params(ui, state, id);
                detect(ui, state, actions, id, kind, frames);
                summary(
                    ui,
                    kind,
                    &channel_name,
                    current,
                    mean,
                    detected,
                    frames,
                    time,
                );
                appearance(ui, state, id);
                ui.row(Block::row().gaps(6.0).align(Align::Center), |ui| {
                    if widget::button(ui, "clear", "Clear", Role::Quiet) {
                        actions.push(Action::ClearObject(id));
                    }
                });
                measure(ui, state, id, time);
                if kind.trackable() {
                    crate::track_panel::build(ui, state, actions, id);
                }
            }
        },
    );
    ui.pop_id();
    if let Some(set) = state.scene.objects.get_mut(id) {
        if set.visible() != visible {
            set.set_visible(visible);
        }
        if set.color() != color {
            set.set_color(color);
        }
    }
}
fn state_card(ui: &mut Ui) -> grana_ui::Look {
    let mut look = ui.theme().look(Role::Card, grana_ui::State::Rest);
    look.corners = Corners::all(11.0);
    look
}

fn params(ui: &mut Ui, state: &mut State<'_>, id: ObjectSetId) {
    let Some(set) = state.scene.objects.get(id) else {
        return;
    };
    let kind = set.kind();
    let voxel = state.scene.dataset.voxel_size().min_element();
    let world = state.scene.dataset.world_size().max_element();
    let mut params = set.params();
    let mut changed = false;
    for (index, field) in kind.fields().iter().enumerate() {
        let (minimum, maximum) = bounds(field, voxel, world);
        let value = params.value(index).clamp(minimum, maximum);
        let readout = format!("{value:.3}{}", field.suffix);
        if let Some(next) = widget::slider_readout(
            ui,
            &format!("param.{index}"),
            Some(field.label),
            value,
            minimum..=maximum,
            field.logarithmic,
            &readout,
        ) {
            params.set_value(index, next.clamp(minimum, maximum));
            changed = true;
        }
    }
    if changed && let Some(set) = state.scene.objects.get_mut(id) {
        set.set_params(params);
    }
}

fn bounds(field: &ParamField, voxel: f32, world: f32) -> (f32, f32) {
    match field.bound {
        ParamBound::Voxel(factor) => (voxel, factor * voxel),
        ParamBound::Unit => (0.0, 1.0),
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
    ui.row(Block::row().gaps(8.0).align(Align::Center), |ui| {
        if running.is_none() && widget::button(ui, "detect", "Detect", Role::Prominent) {
            actions.push(Action::Detect(id));
        }
        if running.is_some() {
            if widget::button(ui, "cancel", "Cancel", Role::Quiet) {
                actions.push(Action::CancelDetect(id));
            }
            widget::spinner(ui, 14.0);
        }
    });
    if let Some(running) = running {
        caption(
            ui,
            &crate::panel::progress_caption(kind, Job::Detect(id), running, frames),
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn summary(
    ui: &mut Ui,
    kind: ObjectKind,
    channel: &str,
    current: Option<(usize, usize)>,
    mean: Option<f32>,
    detected: usize,
    frames: u32,
    time: u32,
) {
    match current {
        Some((count, level)) => {
            widget::body(
                ui,
                &format!("{count} {} · level {}", kind.count_noun(), level + 1),
            );
            if let Some(mean) = mean {
                widget::weak(ui, &format!("mean contrast {mean:.2}"));
            }
        }
        None if detected == 0 => {
            widget::weak(
                ui,
                &format!("{} {channel} in every frame", kind.description()),
            );
            return;
        }
        None => {
            widget::weak(
                ui,
                &format!("no objects on frame {} of {}", time + 1, frames),
            );
        }
    }
    if frames > 1 && detected > 0 {
        widget::weak(ui, &format!("detected in {detected} frames"));
    }
}

fn appearance(ui: &mut Ui, state: &mut State<'_>, id: ObjectSetId) {
    let Some(set) = state.scene.objects.get(id) else {
        return;
    };
    let names = state
        .scene
        .dataset
        .channels()
        .iter()
        .map(|channel| channel.name().to_string())
        .collect::<Vec<_>>();
    let measurements = set
        .colorable_measurements()
        .into_iter()
        .map(|measurement| (measurement, measurement.label(&names)))
        .collect::<Vec<_>>();
    let extent = set.object_extent();
    let mut coloring = set.object_coloring().clone();
    if crate::coloring::edit(
        ui,
        &format!("object.{}.coloring", id.raw()),
        &mut coloring,
        &measurements,
        extent,
        |measurement| (measurement.unit(), measurement.decimals()),
    ) && let Some(set) = state.scene.objects.get_mut(id)
    {
        set.set_object_coloring(coloring);
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
    let Some(frame) = state
        .scene
        .objects
        .get(id)
        .and_then(|object| object.frame(time))
    else {
        return;
    };
    let frame = frame.clone();
    let channel = state
        .scene
        .objects
        .get(id)
        .map_or(0, |object| object.channel());
    match &frame {
        ObjectFrame::Spots(spots) => {
            let Some(index) = selected else {
                widget::weak(ui, "click a spot to measure it");
                return;
            };
            let Some(spot) = spots.spots().get(index).copied() else {
                return;
            };
            let centre = spot.center();
            let intensity = frame.intensities().object(index, channel);
            ui.separator();
            widget::labelled(ui, "object", &format!("#{}", index + 1));
            widget::labelled(
                ui,
                "position",
                &format!("{:.2} · {:.2} · {:.2} µm", centre[0], centre[1], centre[2]),
            );
            widget::labelled(ui, "diameter", &format!("{:.3} µm", spot.radius() * 2.0));
            widget::labelled(ui, "contrast", &format!("{:.3}", spot.quality()));
            widget::labelled(
                ui,
                "intensity",
                &format!(
                    "{:.0} mean · {:.0} … {:.0} · σ {:.0}",
                    intensity.mean(),
                    intensity.minimum(),
                    intensity.maximum(),
                    intensity.deviation()
                ),
            );
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
            object_list(ui, state, id, &entries, "surfaces", |volume| {
                format!("{volume:.1} µm³")
            });
            let Some(object) = measured else {
                widget::weak(ui, "click an object to measure it");
                return;
            };
            let Some(index) = selected else {
                return;
            };
            let intensity = surfaces.intensities().object(index, channel);
            ui.separator();
            widget::labelled(ui, "object", &format!("#{}", index + 1));
            widget::labelled(ui, "volume", &format!("{:.2} µm³", object.volume()));
            widget::labelled(ui, "area", &format!("{:.2} µm²", object.area()));
            widget::labelled(ui, "sphericity", &format!("{:.3}", object.sphericity()));
            let centroid = object.centroid();
            widget::labelled(
                ui,
                "centroid",
                &format!(
                    "{:.2} · {:.2} · {:.2} µm",
                    centroid[0], centroid[1], centroid[2]
                ),
            );
            let size = object.bounds().size();
            widget::labelled(
                ui,
                "extent",
                &format!("{:.2} × {:.2} × {:.2} µm", size[0], size[1], size[2]),
            );
            widget::labelled(ui, "voxels", &object.voxel_count().to_string());
            widget::labelled(
                ui,
                "intensity",
                &format!(
                    "{:.0} mean · {:.0} … {:.0} · σ {:.0}",
                    intensity.mean(),
                    intensity.minimum(),
                    intensity.maximum(),
                    intensity.deviation()
                ),
            );
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
            object_list(ui, state, id, &entries, "filaments", |length| {
                format!("{length:.1} µm")
            });
            let Some(object) = measured else {
                widget::weak(ui, "click a filament to measure it");
                return;
            };
            let Some(index) = selected else {
                return;
            };
            let intensity = filaments.intensities().object(index, channel);
            ui.separator();
            widget::labelled(ui, "object", &format!("#{}", index + 1));
            widget::labelled(ui, "length", &format!("{:.2} µm", object.length()));
            widget::labelled(ui, "diameter", &format!("{:.2} µm", object.diameter()));
            widget::labelled(ui, "volume", &format!("{:.2} µm³", object.volume()));
            widget::labelled(ui, "branches", &object.branches().to_string());
            widget::labelled(ui, "terminals", &object.terminals().to_string());
            widget::labelled(ui, "junctions", &object.junctions().to_string());
            widget::labelled(ui, "nodes", &object.nodes().to_string());
            let centroid = object.centroid();
            widget::labelled(
                ui,
                "centroid",
                &format!(
                    "{:.2} · {:.2} · {:.2} µm",
                    centroid[0], centroid[1], centroid[2]
                ),
            );
            let size = object.bounds().size();
            widget::labelled(
                ui,
                "extent",
                &format!("{:.2} × {:.2} × {:.2} µm", size[0], size[1], size[2]),
            );
            widget::labelled(
                ui,
                "intensity",
                &format!(
                    "{:.0} mean · {:.0} … {:.0} · σ {:.0}",
                    intensity.mean(),
                    intensity.minimum(),
                    intensity.maximum(),
                    intensity.deviation()
                ),
            );
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
    ui.push_id(salt);
    ui.scroll_axis(
        ui.id("list"),
        grana_ui::Axis::Column,
        Length::Fixed(132.0),
        None,
        |ui| {
            for (index, amount) in entries {
                let selected = selection
                    == Some(Selection {
                        set: id,
                        item: Item::Object(*index),
                    });
                let label = format!("#{} · {}", index + 1, value(*amount));
                if row_button(ui, &format!("row.{index}"), &label, selected) {
                    clicked = Some(*index);
                }
            }
        },
    );
    ui.pop_id();
    if let Some(index) = clicked {
        state.scene.select(Some(Selection {
            set: id,
            item: Item::Object(index),
        }));
    }
}

pub fn row_button(ui: &mut Ui, id: &str, label: &str, selected: bool) -> bool {
    let key = ui.id(id);
    let rect = ui.allocate_full(22.0);
    let response = ui.interact(key, rect);
    let hover = ui.animate(
        key,
        if response.hovered { 1.0 } else { 0.0 },
        grana_ui::Motion::quick(),
    );
    let accent = ui.theme().palette.accent;
    let text = ui.theme().palette.text_secondary;
    let style = TextStyle::new(ui.theme().font(grana_ui::Level::Callout), text);
    let mut painter = ui.painter();
    if selected {
        painter.rect(rect, Corners::all(5.0), accent.with_alpha(0.22));
    } else if hover > 0.01 {
        painter.rect(rect, Corners::all(5.0), Color::white_alpha(0.05 * hover));
    }
    painter.text_in(
        Rect::from_min_max(
            Vec2::new(rect.min.x + 6.0, rect.min.y),
            Vec2::new(rect.max.x - 4.0, rect.max.y),
        ),
        label,
        style,
        Align::Center,
    );
    response.clicked
}
