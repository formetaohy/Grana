use glam::Vec2;
use grana_scene::{
    Item, Job, ObjectColor, ObjectFrame, ObjectKind, ObjectSet, ObjectSetId, ParamField,
    ParamScale, Provenance, RelationField, Scene,
};
use grana_ui::{
    Align, Block, Color, Corners, Edges, Icon, Length, Level, Rect, Role, TextStyle, Ui,
};

use crate::panel::{divider, progress, section_header};
use crate::state::State;
use crate::widget;
use grana_session::Command;

pub fn build(ui: &mut Ui, state: &mut State<'_>, commands: &mut Vec<Command>) {
    let channel = state.scene.selected;
    let channel_name = state.scene.channel(channel).name().to_string();
    let count = state.scene.objects.len();
    let add = header(ui, count);
    add_menu(ui, state, commands, add);
    if count == 0 {
        crate::panel::empty_state(
            ui,
            Icon::Layers,
            "No object sets",
            &format!(
                "Detect spots, surfaces or filaments in {channel_name} to measure structures."
            ),
        );
        return;
    }
    let ids = state
        .scene
        .objects
        .iter()
        .map(ObjectSet::id)
        .collect::<Vec<_>>();
    let count = ids.len();
    for (position, id) in ids.into_iter().enumerate() {
        object(ui, state, commands, id, position, count);
    }
}

fn header(ui: &mut Ui, count: usize) -> Rect {
    let row = ui.allocate_full(24.0);
    let palette = ui.theme().palette;
    let add = Rect::from_center_size(
        Vec2::new(row.max.x - 11.0, row.center().y),
        Vec2::splat(22.0),
    );
    let label = TextStyle::new(ui.theme().label_font(), palette.text_tertiary);
    let detail =
        TextStyle::new(ui.theme().font(Level::Caption), palette.text_tertiary).aligned(Align::End);
    {
        let mut painter = ui.painter();
        painter.text_in(row, "Object sets", label, Align::Center);
        painter.text_in(
            Rect::from_min_max(row.min, Vec2::new(add.min.x - 8.0, row.max.y)),
            &count.to_string(),
            detail,
            Align::Center,
        );
    }
    let response = ui.interact(ui.id("objects.add"), add);
    if response.clicked {
        ui.toggle_popup(ui.id("objects.add"));
    }
    let hover = ui.animate(
        ui.id("objects.add.hover"),
        if response.hovered { 1.0 } else { 0.0 },
        grana_ui::Motion::quick(),
    );
    {
        let mut painter = ui.painter();
        if hover > 0.01 {
            painter.rect(add, Corners::all(4.0), palette.raised);
        }
        grana_ui::icon::paint(
            Icon::Plus,
            &mut painter,
            add.center(),
            13.0,
            palette.text_secondary.lerp(palette.text_primary, hover),
            1.6,
        );
    }
    add
}

fn add_menu(ui: &mut Ui, state: &State<'_>, commands: &mut Vec<Command>, anchor: Rect) {
    let open = ui.popup_open(ui.id("objects.add"));
    let mut chosen = None;
    grana_ui::popover::popover(ui, "objects.add", open, anchor, 184.0, |ui| {
        if grana_ui::popover::action_row(ui, "add.spots", Icon::Target, "Spots", None) {
            chosen = Some(ObjectKind::Spots);
        }
        if grana_ui::popover::action_row(ui, "add.surfaces", Icon::Cube, "Surfaces", None) {
            chosen = Some(ObjectKind::Surfaces);
        }
        if grana_ui::popover::action_row(ui, "add.filaments", Icon::Channel, "Filaments", None) {
            chosen = Some(ObjectKind::Filaments);
        }
    });
    if let Some(kind) = chosen {
        ui.close_popup();
        if state.io.loading.is_none() {
            commands.push(Command::AddObject(kind));
        }
    }
}

fn kind_icon(kind: ObjectKind) -> Icon {
    match kind {
        ObjectKind::Spots => Icon::Target,
        ObjectKind::Surfaces => Icon::Cube,
        ObjectKind::Filaments => Icon::Channel,
    }
}

fn object(
    ui: &mut Ui,
    state: &mut State<'_>,
    commands: &mut Vec<Command>,
    id: ObjectSetId,
    position: usize,
    sets: usize,
) {
    let Some(set) = state.scene.objects.get(id) else {
        return;
    };
    let kind = set.kind();
    let name = set.name().to_string();
    let channel = set.channel();
    let channel_name = state.scene.dataset.channels()[channel].name().to_string();
    let color = set.color();
    let mut visible = set.visible();
    let mut labels = set.labels();
    let mut opacity = set.opacity();
    let detected = set.detected();
    let merged = set.provenance().is_merged();
    let frames = state.scene.frame_count();
    let time = state.scene.time();
    let current = set
        .frame(time)
        .map(|frame| (frame.object_count(), frame.level()));
    let mean = set
        .frame(time)
        .and_then(ObjectFrame::spots)
        .and_then(|spots| spots.mean_quality());
    let mut label_metric = set.label_metric();
    let label_options = label_metrics(state.scene, set);
    let hidden = set.hidden_count();
    let selected = state
        .scene
        .selection()
        .and_then(|selection| match selection.item {
            Item::Object(object) if selection.set == id && set.visible_object(time, object) => {
                Some(object)
            }
            _ => None,
        });
    let open_id = format!("object.{}.open", id.raw());
    let expanded = ui.disclosure_open(&open_id, false);
    let palette = ui.theme().palette;
    let tint = Color::from_rgb_f32(color);
    let look = ui.theme().look(Role::Card, grana_ui::State::Rest);
    ui.push_id(&format!("object.{}", id.raw()));
    ui.column(
        Block::column()
            .padding(Edges::all(10.0))
            .gaps(6.0)
            .background(look),
        |ui| {
            let header = ui.allocate_full(38.0);
            let icon_rect = Rect::from_center_size(
                Vec2::new(header.min.x + 14.0, header.center().y),
                Vec2::splat(28.0),
            );
            let menu_rect = Rect::from_center_size(
                Vec2::new(header.max.x - 11.0, header.center().y),
                Vec2::splat(22.0),
            );
            let eye_rect = Rect::from_center_size(
                Vec2::new(menu_rect.min.x - 22.0, header.center().y),
                Vec2::splat(22.0),
            );
            let text_area = Rect::from_min_max(
                Vec2::new(icon_rect.max.x + 9.0, header.min.y),
                Vec2::new(eye_rect.min.x - 22.0, header.max.y),
            );
            let hit =
                Rect::from_min_max(text_area.min, Vec2::new(eye_rect.min.x - 8.0, header.max.y));
            let key = ui.id("header");
            let mut response = ui.interact(key, hit);
            if ui.focusable(key, hit, Corners::all(6.0)) && ui.activated(key) {
                response.clicked = true;
            }
            if response.clicked {
                ui.store_open(&open_id, !expanded);
            }
            let name_rect = Rect::from_min_max(
                Vec2::new(text_area.min.x, text_area.min.y + 2.0),
                Vec2::new(text_area.max.x, text_area.center().y + 1.0),
            );
            let subtitle = match current {
                Some((count, _level)) => {
                    if frames > 1 {
                        format!(
                            "{channel_name} · {count} {} · {detected} frames",
                            kind.count_noun()
                        )
                    } else {
                        format!("{channel_name} · {count} {}", kind.count_noun())
                    }
                }
                None if detected > 0 => {
                    format!("{channel_name} · none on frame {}", time + 1)
                }
                None => format!("{channel_name} · not detected"),
            };
            let subtitle = if hidden > 0 {
                format!("{subtitle} · {hidden} hidden")
            } else {
                subtitle
            };
            let name_style =
                TextStyle::new(ui.theme().font(Level::Body).medium(), palette.text_primary)
                    .truncating();
            let subtitle_style = TextStyle::new(
                grana_ui::TypeStyle::new(10.5, 400.0, 13.0),
                palette.text_tertiary,
            )
            .truncating();
            let editing = ui.is_editing(ui.id("name"));
            let arrow = ui.animate(
                ui.id("arrow"),
                if expanded { 1.0 } else { 0.0 },
                grana_ui::Motion::snappy(),
            );
            {
                let mut painter = ui.painter();
                painter.rect(icon_rect, Corners::all(6.0), tint.with_alpha(0.12));
                painter.outlined(
                    icon_rect,
                    Corners::all(6.0),
                    grana_ui::Fill::None,
                    grana_ui::Border {
                        width: 1.0,
                        color: tint.with_alpha(0.26),
                    },
                    grana_ui::Shadow::NONE,
                );
                grana_ui::icon::paint(
                    kind_icon(kind),
                    &mut painter,
                    icon_rect.center(),
                    14.0,
                    tint,
                    1.5,
                );
                if !editing {
                    painter.text_in(name_rect, &name, name_style, Align::Center);
                }
                painter.text_in(
                    Rect::from_min_max(
                        Vec2::new(text_area.min.x, text_area.center().y + 3.0),
                        Vec2::new(text_area.max.x, text_area.max.y),
                    ),
                    &subtitle,
                    subtitle_style,
                    Align::Center,
                );
                let centre = Vec2::new(text_area.max.x + 8.0, header.center().y);
                let angle = std::f32::consts::FRAC_PI_2 * (0.5 + 0.5 * arrow);
                let tip = centre + Vec2::new(angle.cos(), angle.sin()) * 2.2;
                painter.segment(tip + Vec2::new(-2.0, -1.3), tip, 1.4, palette.text_tertiary);
                painter.segment(tip, tip + Vec2::new(2.0, -1.3), 1.4, palette.text_tertiary);
            }
            if editing
                && let Some((outcome, text)) = widget::text_field(ui, "name", name_rect, name_style)
                && outcome == grana_ui::EditOutcome::Commit
                && text.trim() != name
            {
                commands.push(Command::RenameObject {
                    set: id,
                    name: text.trim().to_string(),
                });
            }
            ui.place(eye_rect, |ui| {
                let tooltip = if visible {
                    "Hide object set"
                } else {
                    "Show object set"
                };
                if widget::icon_button(
                    ui,
                    "visible",
                    if visible { Icon::Eye } else { Icon::EyeOff },
                    22.0,
                    Role::Quiet,
                    tooltip,
                ) {
                    visible = !visible;
                }
            });
            let menu_key = ui.id("menu");
            ui.place(menu_rect, |ui| {
                if widget::icon_button(
                    ui,
                    "menu.button",
                    Icon::Ellipsis,
                    22.0,
                    Role::Quiet,
                    "Object commands",
                ) {
                    ui.toggle_popup(menu_key);
                }
                let open = ui.popup_open(menu_key);
                let mut rename = false;
                let mut duplicate = false;
                let mut merge = None;
                let mut remove = false;
                let mut reorder = None;
                let merges = state
                    .scene
                    .objects
                    .iter()
                    .filter(|candidate| candidate.id() != id && candidate.kind() == kind)
                    .map(|candidate| (candidate.id(), candidate.name().to_string()))
                    .collect::<Vec<_>>();
                grana_ui::popover::popover(ui, "object.menu", open, menu_rect, 184.0, |ui| {
                    if grana_ui::popover::action_row(ui, "rename", Icon::Pencil, "Rename", None) {
                        rename = true;
                    }
                    if grana_ui::popover::action_row(
                        ui,
                        "duplicate",
                        Icon::Layers,
                        "Duplicate",
                        None,
                    ) {
                        duplicate = true;
                    }
                    if !merges.is_empty() {
                        grana_ui::popover::separator(ui);
                        widget::caption(ui, "Merge into");
                        for (index, (candidate, label)) in merges.iter().enumerate() {
                            if grana_ui::popover::menu_row(ui, index, label, false) {
                                merge = Some(*candidate);
                            }
                        }
                    }
                    grana_ui::popover::separator(ui);
                    if position > 0
                        && grana_ui::popover::action_row(
                            ui,
                            "move.up",
                            Icon::ChevronUp,
                            "Move up",
                            None,
                        )
                    {
                        reorder = Some(position - 1);
                    }
                    if position + 1 < sets
                        && grana_ui::popover::action_row(
                            ui,
                            "move.down",
                            Icon::ChevronDown,
                            "Move down",
                            None,
                        )
                    {
                        reorder = Some(position + 1);
                    }
                    if grana_ui::popover::action_row(
                        ui,
                        "remove",
                        Icon::Trash,
                        "Remove object set",
                        None,
                    ) {
                        remove = true;
                    }
                });
                if rename {
                    ui.close_popup();
                    ui.begin_edit(ui.id("name"), &name);
                }
                if duplicate {
                    ui.close_popup();
                    commands.push(Command::DuplicateObject(id));
                }
                if let Some(target) = merge {
                    ui.close_popup();
                    commands.push(Command::MergeObjects { source: id, target });
                }
                if let Some(index) = reorder {
                    ui.close_popup();
                    commands.push(Command::ReorderObject { set: id, index });
                }
                if remove {
                    ui.close_popup();
                    commands.push(Command::RemoveObject(id));
                }
            });
            if expanded {
                divider(ui);
                if let Some(value) = widget::slider_readout(
                    ui,
                    "opacity",
                    Some("Opacity"),
                    opacity,
                    0.0..=1.0,
                    false,
                    &format!("{:.0}%", opacity * 100.0),
                ) {
                    opacity = value;
                }
                if let Some(value) = widget::toggle_row(ui, "labels", "Object labels", labels) {
                    labels = value;
                }
                if labels {
                    ui.row(Block::row().gaps(8.0).align(Align::Center), |ui| {
                        widget::field_label(ui, "label by");
                        if let Some(chosen) = grana_ui::popover::combo(
                            ui,
                            "label.metric",
                            168.0,
                            &label_options,
                            Some(label_metric),
                        ) {
                            label_metric = chosen;
                        }
                    });
                }
                if hidden > 0 || selected.is_some() {
                    ui.row(Block::row().gaps(6.0).align(Align::Center), |ui| {
                        if hidden > 0 {
                            widget::caption(ui, &format!("{hidden} hidden"));
                            if widget::button(ui, "show.all", "Show all", grana_ui::Role::Quiet) {
                                commands.push(Command::ClearHidden(id));
                            }
                        }
                        if let Some(object) = selected
                            && widget::button(
                                ui,
                                "hide.selected",
                                "Hide selected",
                                grana_ui::Role::Quiet,
                            )
                        {
                            commands.push(Command::SetObjectHidden {
                                set: id,
                                time,
                                objects: vec![object],
                                hidden: true,
                            });
                        }
                    });
                }
                origin(ui, state, id);
                curated(ui, state, id);
                if !merged {
                    params(ui, state, commands, id);
                    if kind == ObjectKind::Surfaces {
                        seeds(ui, state, commands, id);
                    }
                    detect(ui, state, commands, id, kind, frames);
                }
                summary(ui, kind, current, mean, detected, frames, time);
                measure(ui, state, commands, id, time);
                appearance(ui, state, commands, id);
                relations(ui, state, commands, id);
                if kind.trackable() {
                    crate::track_panel::entry(ui, state, id);
                }
                if detected > 0 {
                    ui.row(Block::row().gaps(6.0).align(Align::Center), |ui| {
                        if widget::button(ui, "clear", "Clear objects", Role::Danger) {
                            commands.push(Command::ClearObject(id));
                        }
                    });
                }
            }
        },
    );
    ui.pop_id();
    if let Some(set) = state.scene.objects.get(id) {
        if set.visible() != visible {
            commands.push(Command::SetObjectVisible { set: id, visible });
        }
        if set.labels() != labels {
            commands.push(Command::SetObjectLabels { set: id, labels });
        }
        if set.label_metric() != label_metric {
            commands.push(Command::SetLabelMetric {
                set: id,
                metric: label_metric,
            });
        }
        if set.opacity() != opacity {
            commands.push(Command::SetObjectOpacity {
                set: id,
                opacity: opacity.clamp(0.0, 1.0),
            });
        }
    }
}

fn label_metrics(scene: &Scene, set: &ObjectSet) -> Vec<(Option<ObjectColor>, String)> {
    let channels = scene
        .dataset
        .channels()
        .iter()
        .map(|channel| channel.name().to_string())
        .collect::<Vec<_>>();
    let mut options = vec![(None, "Number".to_string())];
    options.extend(set.colorable_measurements().into_iter().map(|measurement| {
        (
            Some(ObjectColor::Measurement(measurement)),
            measurement.label(&channels),
        )
    }));
    for relation in set.relations() {
        let Some(target) = scene.objects.get(relation.target()) else {
            continue;
        };
        for field in RelationField::CONTINUOUS {
            let mut label = format!("{} {}", target.name(), field.label());
            if relation.shift() != 0 {
                label.push_str(&format!(" · t{:+}", relation.shift()));
            }
            options.push((
                Some(ObjectColor::Relation {
                    target: relation.target(),
                    field,
                    shift: relation.shift(),
                }),
                label,
            ));
        }
    }
    options
}

fn relations(ui: &mut Ui, state: &State<'_>, commands: &mut Vec<Command>, id: ObjectSetId) {
    let Some(set) = state.scene.objects.get(id) else {
        return;
    };
    let frames = state.scene.frame_count();
    let targets = state
        .scene
        .objects
        .iter()
        .filter(|other| other.detected() > 0)
        .map(|other| (other.id(), other.name().to_string()))
        .collect::<Vec<_>>();
    if targets.is_empty() && set.relations().is_empty() {
        return;
    }
    let relations = set
        .relations()
        .iter()
        .map(|relation| {
            (
                relation.target(),
                relation.shift(),
                relation.computed(),
                relation.frame_count(),
            )
        })
        .collect::<Vec<_>>();
    let open = ui.disclosure_open("relations", false);
    let next = widget::disclosure(ui, "relations", "Relations", open, |ui| {
        for (target, shift, computed, total) in relations.iter().copied() {
            let name = targets
                .iter()
                .find(|(candidate, _)| *candidate == target)
                .map_or("another set", |(_, name)| name.as_str());
            let running = progress(
                state.jobs,
                Job::Relation {
                    source: id,
                    target,
                    shift,
                },
            );
            let taken = relations
                .iter()
                .filter(|(candidate, _, _, _)| *candidate == target)
                .map(|(_, taken, _, _)| *taken)
                .collect::<Vec<_>>();
            ui.row(Block::row().gaps(6.0).align(Align::Center), |ui| {
                let mut label = name.to_string();
                if shift != 0 {
                    label.push_str(&format!(" · t{shift:+}"));
                }
                widget::caption(ui, &label);
                ui.space(1.0);
                let (done, total) = match running {
                    Some(running) => (running.done, running.total),
                    None => (computed, total),
                };
                widget::caption(ui, &format!("{done}/{total}"));
                if widget::icon_button(
                    ui,
                    &format!("relation.{}.{}.remove", target.raw(), shift),
                    Icon::Close,
                    22.0,
                    Role::Quiet,
                    "Remove the relation",
                ) {
                    commands.push(Command::RemoveRelation {
                        source: id,
                        target,
                        shift,
                    });
                }
            });
            if frames > 1 {
                let limit = frames as i32 - 1;
                let mut next_shift = None;
                ui.row(Block::row().gaps(4.0).align(Align::Center), |ui| {
                    widget::field_label(ui, "shift");
                    if widget::icon_button(
                        ui,
                        &format!("relation.{}.{}.back", target.raw(), shift),
                        Icon::ChevronLeft,
                        20.0,
                        Role::Quiet,
                        "Relate to the target one frame earlier",
                    ) && shift > -limit
                        && !taken.contains(&(shift - 1))
                    {
                        next_shift = Some(shift - 1);
                    }
                    widget::caption(ui, &shift_label(shift));
                    if widget::icon_button(
                        ui,
                        &format!("relation.{}.{}.forward", target.raw(), shift),
                        Icon::ChevronRight,
                        20.0,
                        Role::Quiet,
                        "Relate to the target one frame later",
                    ) && shift < limit
                        && !taken.contains(&(shift + 1))
                    {
                        next_shift = Some(shift + 1);
                    }
                    ui.space(1.0);
                    let free = free_offsets(target, id, limit, &taken);
                    if !free.is_empty() {
                        let key = format!("relation.{}.{}.offset", target.raw(), shift);
                        let response = widget::icon_button_response(
                            ui,
                            &key,
                            Icon::Plus,
                            20.0,
                            Role::Quiet,
                            "Relate to the target at another offset",
                        );
                        if response.clicked {
                            ui.toggle_popup(ui.id(&key));
                        }
                        let open = ui.popup_open(ui.id(&key));
                        let mut chosen = None;
                        grana_ui::popover::popover(ui, &key, open, response.rect, 176.0, |ui| {
                            for (index, offset) in free.iter().enumerate() {
                                if grana_ui::popover::menu_row(
                                    ui,
                                    index,
                                    &shift_label(*offset),
                                    false,
                                ) {
                                    chosen = Some(*offset);
                                }
                            }
                        });
                        if let Some(offset) = chosen {
                            ui.close_popup();
                            commands.push(Command::AddRelation {
                                source: id,
                                target,
                                shift: offset,
                            });
                        }
                    }
                });
                if let Some(next_shift) = next_shift {
                    commands.push(Command::MoveRelation {
                        source: id,
                        target,
                        shift,
                        next: next_shift,
                    });
                }
            }
        }
        let unlinked = targets
            .iter()
            .filter(|(target, _)| {
                !relations
                    .iter()
                    .any(|(candidate, _, _, _)| candidate == target)
                    && (*target != id || frames > 1)
            })
            .map(|(target, name)| {
                let label = if *target == id {
                    format!("{name} · earlier frames")
                } else {
                    name.clone()
                };
                (*target, label)
            })
            .collect::<Vec<_>>();
        if !unlinked.is_empty() {
            let response = widget::button_with(
                ui,
                "relation.add",
                "Relate to an object set",
                Role::Control,
                Length::Fill,
            );
            let anchor = response.rect;
            if response.clicked {
                ui.toggle_popup(ui.id("relation.menu"));
            }
            let open = ui.popup_open(ui.id("relation.menu"));
            let mut chosen = None;
            grana_ui::popover::popover(ui, "relation.menu", open, anchor, 208.0, |ui| {
                for (index, (target, label)) in unlinked.iter().enumerate() {
                    if grana_ui::popover::menu_row(ui, index, label, false) {
                        chosen = Some(*target);
                    }
                }
            });
            if let Some(target) = chosen {
                ui.close_popup();
                commands.push(Command::AddRelation {
                    source: id,
                    target,
                    shift: if target == id { -1 } else { 0 },
                });
            }
        }
    });
    ui.store_open("relations", next);
}

fn free_offsets(target: ObjectSetId, source: ObjectSetId, limit: i32, taken: &[i32]) -> Vec<i32> {
    let mut free = (-limit..=limit)
        .filter(|offset| !taken.contains(offset) && (target != source || *offset != 0))
        .collect::<Vec<_>>();
    free.sort_by_key(|offset| (offset.abs(), *offset));
    free
}

fn shift_label(shift: i32) -> String {
    match shift {
        0 => "same frame".to_string(),
        1 => "target +1 frame".to_string(),
        -1 => "target −1 frame".to_string(),
        value if value > 0 => format!("target +{value} frames"),
        value => format!("target {value} frames"),
    }
}

fn origin(ui: &mut Ui, state: &State<'_>, id: ObjectSetId) {
    let Some(set) = state.scene.objects.get(id) else {
        return;
    };
    let name = |id: ObjectSetId| {
        state
            .scene
            .objects
            .get(id)
            .map_or("an object set".to_string(), |set| set.name().to_string())
    };
    let text = match set.provenance() {
        Provenance::Detection => return,
        Provenance::Filtered { source, .. } => format!("Filtered from {}", name(*source)),
        Provenance::Copied { source } => format!("Copy of {}", name(*source)),
        Provenance::Merged { parts } => format!(
            "Merged from {}",
            parts
                .iter()
                .map(|part| part.name())
                .collect::<Vec<_>>()
                .join(" + ")
        ),
    };
    widget::labelled(ui, "Origin", &text);
}

fn curated(ui: &mut Ui, state: &State<'_>, id: ObjectSetId) {
    let Some(set) = state.scene.objects.get(id) else {
        return;
    };
    let curation = set.curation();
    if curation.is_empty() && curation.dropped() == 0 {
        return;
    }
    let mut parts = Vec::new();
    let removed = curation.removal_count();
    if removed > 0 {
        parts.push(format!(
            "{removed} object{} removed by hand",
            if removed == 1 { "" } else { "s" }
        ));
    }
    let edits = curation.edit_count();
    if edits > 0 {
        parts.push(format!(
            "{edits} track edit{}",
            if edits == 1 { "" } else { "s" }
        ));
    }
    let pending = curation.pending();
    if pending > 0 {
        parts.push(format!("{pending} awaiting a matching object"));
    }
    let dropped = curation.dropped();
    if dropped > 0 {
        parts.push(format!("{dropped} dropped after a rebuild"));
    }
    widget::labelled(ui, "Curated", &parts.join(" · "));
}

fn params(ui: &mut Ui, state: &State<'_>, commands: &mut Vec<Command>, id: ObjectSetId) {
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
        let label = capitalized(field.label);
        if let Some(next) = widget::slider_readout(
            ui,
            &format!("param.{index}"),
            Some(&label),
            value,
            minimum..=maximum,
            field.logarithmic,
            &readout,
        ) {
            params.set_value(index, next.clamp(minimum, maximum));
            changed = true;
        }
    }
    if changed {
        commands.push(Command::SetObjectParams { set: id, params });
    }
}

fn capitalized(label: &str) -> String {
    let mut characters = label.chars();
    match characters.next() {
        Some(first) => first.to_uppercase().collect::<String>() + characters.as_str(),
        None => String::new(),
    }
}

fn bounds(field: &ParamField, voxel: f32, world: f32) -> (f32, f32) {
    (
        scale(field.minimum, voxel, world),
        scale(field.maximum, voxel, world),
    )
}

fn scale(scale: ParamScale, voxel: f32, world: f32) -> f32 {
    match scale {
        ParamScale::Voxel(factor) => factor * voxel,
        ParamScale::World(fraction) => fraction * world,
        ParamScale::Absolute(value) => value,
    }
}

fn detect(
    ui: &mut Ui,
    state: &State<'_>,
    commands: &mut Vec<Command>,
    id: ObjectSetId,
    kind: ObjectKind,
    frames: u32,
) {
    let root = state.scene.detection_root(id).unwrap_or(id);
    let root_name = state
        .scene
        .objects
        .get(root)
        .map_or_else(String::new, |set| set.name().to_string());
    let running = progress(state.jobs, Job::Detect(root))
        .map(|running| (Job::Detect(root), running))
        .or_else(|| {
            progress(state.jobs, Job::Recut(root)).map(|running| (Job::Recut(root), running))
        });
    match running {
        Some((job, running)) => {
            ui.row(Block::row().gaps(6.0).align(Align::Center), |ui| {
                widget::spinner(ui, 13.0);
                widget::caption(
                    ui,
                    &crate::panel::progress_caption(kind, job, running, frames),
                );
            });
            let fraction = if running.total > 0 {
                running.done as f32 / running.total as f32
            } else {
                0.0
            };
            widget::progress(ui, fraction, ui.available().x);
            if widget::button(ui, "cancel", "Cancel", Role::Control) {
                commands.push(Command::CancelDetect(id));
            }
        }
        None => {
            let label = if root == id {
                "Detect".to_string()
            } else {
                format!("Detect {root_name}")
            };
            if widget::button_with(ui, "detect", &label, Role::Prominent, Length::Fill).clicked {
                commands.push(Command::Detect(id));
            }
        }
    }
}

fn seeds(ui: &mut Ui, state: &mut State<'_>, commands: &mut Vec<Command>, id: ObjectSetId) {
    let source = state.scene.seed_source(id);
    let count = state.scene.seed_count(id);
    let candidates = state
        .scene
        .objects
        .iter()
        .filter(|set| set.id() != id && (set.detected() > 0 || Some(set.id()) == source))
        .filter(|set| !state.scene.depends_on(set.id(), id))
        .map(|set| (set.id(), set.name().to_string()))
        .collect::<Vec<_>>();
    let mut options = vec![(0usize, "Manual seeds".to_string())];
    options.extend(
        candidates
            .iter()
            .enumerate()
            .map(|(index, (_, name))| (index + 1, format!("From {name}"))),
    );
    let selected = source.and_then(|source| {
        candidates
            .iter()
            .position(|(candidate, _)| *candidate == source)
            .map(|position| position + 1)
    });
    let width = ui.available().x;
    if let Some(index) =
        grana_ui::popover::combo(ui, "seed.source", width, &options, selected.or(Some(0)))
    {
        let chosen = (index > 0).then(|| candidates[index - 1].0);
        if chosen != source {
            commands.push(Command::SetSeedSource {
                set: id,
                source: chosen,
            });
        }
    }
    if let Some(source) = source {
        state.workspace.seed = None;
        let name = candidates
            .iter()
            .find(|(candidate, _)| *candidate == source)
            .map_or("the seed source", |(_, name)| name.as_str());
        ui.row(Block::row().gaps(6.0).align(Align::Center), |ui| {
            widget::field_label(ui, "Seeds");
            widget::caption(ui, &format!("{count} from {name}"));
        });
        widget::weak(ui, "One surface object per seeded object.");
        return;
    }
    let armed = state.workspace.seed == Some(id);
    ui.row(Block::row().gaps(6.0).align(Align::Center), |ui| {
        widget::field_label(ui, "Seeds");
        widget::caption(ui, &format!("{count}"));
        ui.space(1.0);
        let label = if armed { "Stop placing" } else { "Place seeds" };
        let role = if armed { Role::Tinted } else { Role::Control };
        if widget::button(ui, "seed.arm", label, role) {
            state.workspace.seed = (!armed).then_some(id);
        }
        if count > 0
            && widget::icon_button(
                ui,
                "seed.clear",
                Icon::Trash,
                22.0,
                Role::Quiet,
                "Clear seeds",
            )
        {
            commands.push(Command::ClearSeeds(id));
        }
    });
    if armed {
        widget::caption(ui, "Click a surface to seed it · option-click removes");
    }
}

fn summary(
    ui: &mut Ui,
    kind: ObjectKind,
    current: Option<(usize, usize)>,
    mean: Option<f32>,
    detected: usize,
    frames: u32,
    time: u32,
) {
    match current {
        Some((count, level)) => {
            let mut text = format!("{count} {} · level {}", kind.count_noun(), level + 1);
            if let Some(mean) = mean {
                text.push_str(&format!(" · contrast {mean:.2}"));
            }
            widget::caption(ui, &text);
        }
        None if detected == 0 => {
            widget::weak(ui, &format!("{} {}", kind.description(), "in every frame."));
        }
        None => {
            widget::weak(
                ui,
                &format!("No objects on frame {} of {frames}.", time + 1),
            );
        }
    }
}

fn appearance(ui: &mut Ui, state: &mut State<'_>, commands: &mut Vec<Command>, id: ObjectSetId) {
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
    let mut metrics = set
        .colorable_measurements()
        .into_iter()
        .map(|measurement| {
            (
                grana_scene::ObjectColor::Measurement(measurement),
                measurement.label(&names),
            )
        })
        .collect::<Vec<_>>();
    for relation in set.relations() {
        let target = state
            .scene
            .objects
            .get(relation.target())
            .map_or_else(|| "target".to_string(), |target| target.name().to_string());
        let shift = match relation.shift() {
            0 => String::new(),
            shift => format!(" · t{shift:+}"),
        };
        for field in RelationField::CONTINUOUS {
            let metric = grana_scene::ObjectColor::Relation {
                target: relation.target(),
                field,
                shift: relation.shift(),
            };
            metrics.push((metric, format!("{target} · {}{shift}", field.label())));
        }
    }
    let extent = set.object_extent();
    let mut coloring = set.object_coloring().clone();
    let open = ui.disclosure_open("coloring", false);
    let next = widget::disclosure(ui, "coloring", "Coloring", open, |ui| {
        if crate::coloring::edit(
            ui,
            &format!("object.{}.coloring", id.raw()),
            &mut coloring,
            &metrics,
            extent,
            |metric| (metric.unit(), metric.decimals()),
        ) {
            commands.push(Command::SetObjectColoring { set: id, coloring });
        }
    });
    ui.store_open("coloring", next);
}

fn measure(
    ui: &mut Ui,
    state: &mut State<'_>,
    commands: &mut Vec<Command>,
    id: ObjectSetId,
    time: u32,
) {
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
    section_header(ui, "Measurements", None);
    match &frame {
        ObjectFrame::Spots(spots) => {
            let Some(index) = selected else {
                widget::weak(ui, "Click a spot to measure it.");
                return;
            };
            let Some(spot) = spots.spots().get(index).copied() else {
                return;
            };
            let centre = spot.center();
            let intensity = frame.intensities().object(index, channel);
            widget::labelled(ui, "Object", &format!("#{}", index + 1));
            widget::labelled(
                ui,
                "Position",
                &format!("{:.2} · {:.2} · {:.2} µm", centre[0], centre[1], centre[2]),
            );
            widget::labelled(ui, "Diameter", &format!("{:.3} µm", spot.radius() * 2.0));
            widget::labelled(ui, "Contrast", &format!("{:.3}", spot.quality()));
            widget::labelled(
                ui,
                "Intensity",
                &format!(
                    "{:.0} mean · {:.0}–{:.0} · σ {:.0}",
                    intensity.mean(),
                    intensity.minimum(),
                    intensity.maximum(),
                    intensity.deviation()
                ),
            );
        }
        ObjectFrame::Surfaces(surfaces) => {
            let Some(index) = selected else {
                widget::weak(ui, "Click an object to measure it.");
                return;
            };
            let Some(object) = surfaces.objects().get(index).copied() else {
                return;
            };
            let intensity = surfaces.intensities().object(index, channel);
            widget::labelled(ui, "Object", &format!("#{}", index + 1));
            widget::labelled(ui, "Volume", &format!("{:.2} µm³", object.volume()));
            widget::labelled(ui, "Area", &format!("{:.2} µm²", object.area()));
            widget::labelled(ui, "Sphericity", &format!("{:.3}", object.sphericity()));
            let centroid = object.centroid();
            widget::labelled(
                ui,
                "Centroid",
                &format!(
                    "{:.2} · {:.2} · {:.2} µm",
                    centroid[0], centroid[1], centroid[2]
                ),
            );
            let size = object.bounds().size();
            widget::labelled(
                ui,
                "Extent",
                &format!("{:.2} × {:.2} × {:.2} µm", size[0], size[1], size[2]),
            );
            widget::labelled(ui, "Voxels", &object.voxel_count().to_string());
            widget::labelled(
                ui,
                "Intensity",
                &format!(
                    "{:.0} mean · {:.0}–{:.0} · σ {:.0}",
                    intensity.mean(),
                    intensity.minimum(),
                    intensity.maximum(),
                    intensity.deviation()
                ),
            );
        }
        ObjectFrame::Filaments(filaments) => {
            let Some(index) = selected else {
                widget::weak(ui, "Click a filament to measure it.");
                return;
            };
            let Some(object) = filaments.object(index) else {
                return;
            };
            let intensity = filaments.intensities().object(index, channel);
            widget::labelled(ui, "Object", &format!("#{}", index + 1));
            widget::labelled(ui, "Length", &format!("{:.2} µm", object.length()));
            widget::labelled(ui, "Diameter", &format!("{:.2} µm", object.diameter()));
            widget::labelled(ui, "Volume", &format!("{:.2} µm³", object.volume()));
            widget::labelled(ui, "Branches", &object.branches().to_string());
            widget::labelled(ui, "Terminals", &object.terminals().to_string());
            widget::labelled(ui, "Junctions", &object.junctions().to_string());
            widget::labelled(ui, "Nodes", &object.nodes().to_string());
            let centroid = object.centroid();
            widget::labelled(
                ui,
                "Centroid",
                &format!(
                    "{:.2} · {:.2} · {:.2} µm",
                    centroid[0], centroid[1], centroid[2]
                ),
            );
            let size = object.bounds().size();
            widget::labelled(
                ui,
                "Extent",
                &format!("{:.2} × {:.2} × {:.2} µm", size[0], size[1], size[2]),
            );
            widget::labelled(
                ui,
                "Intensity",
                &format!(
                    "{:.0} mean · {:.0}–{:.0} · σ {:.0}",
                    intensity.mean(),
                    intensity.minimum(),
                    intensity.maximum(),
                    intensity.deviation()
                ),
            );
        }
    }
    if let Some(object) = selected
        && widget::button_with(ui, "delete", "Delete object", Role::Danger, Length::Fill).clicked
    {
        commands.push(Command::DeleteObject {
            set: id,
            time,
            object,
        });
    }
}
