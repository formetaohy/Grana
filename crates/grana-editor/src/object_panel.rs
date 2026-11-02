use glam::Vec2;
use grana_scene::{
    Item, Job, ObjectFrame, ObjectKind, ObjectSet, ObjectSetId, ParamBound, ParamField, Selection,
};
use grana_ui::{Align, Block, Color, Corners, Edges, Length, Rect, Role, TextStyle, Ui};

use crate::panel::{caption, progress};
use crate::state::State;
use crate::widget;
use grana_session::Command;

pub fn build(ui: &mut Ui, state: &mut State<'_>, commands: &mut Vec<Command>) {
    let channel = state.scene.selected;
    let channel_name = state.scene.channel(channel).name().to_string();
    let count = state.scene.objects.len();
    let add = section_header(ui, count);
    add_menu(ui, state, commands, add);
    if count == 0 {
        empty(ui, &channel_name);
        return;
    }
    let ids = state
        .scene
        .objects
        .iter()
        .map(ObjectSet::id)
        .collect::<Vec<_>>();
    for id in ids {
        object(ui, state, commands, id);
    }
    widget::weak(ui, &format!("new objects read {channel_name}"));
}

fn section_header(ui: &mut Ui, count: usize) -> Rect {
    let header = ui.allocate_full(26.0);
    let detail = format!("{count}");
    let style = TextStyle::new(
        grana_ui::TypeStyle::new(10.0, 600.0, 13.0),
        ui.theme().palette.text_tertiary,
    );
    let detail_style = TextStyle::new(
        grana_ui::TypeStyle::new(10.0, 400.0, 13.0),
        ui.theme().palette.text_tertiary,
    )
    .aligned(Align::End);
    let add = Rect::from_center_size(
        Vec2::new(header.max.x - 12.0, header.center().y),
        Vec2::splat(24.0),
    );
    {
        let mut painter = ui.painter();
        painter.text_in(header, "OBJECTS", style, Align::Center);
        painter.text_in(
            Rect::from_min_max(header.min, Vec2::new(header.max.x - 30.0, header.max.y)),
            &detail,
            detail_style,
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
    let icon_color = ui.theme().palette.text_secondary;
    {
        let mut painter = ui.painter();
        if hover > 0.01 {
            painter.rect(add, Corners::capsule(), Color::white_alpha(0.08 * hover));
        }
        grana_ui::icon::paint(
            grana_ui::Icon::Plus,
            &mut painter,
            add.center(),
            13.0,
            icon_color,
            1.6,
        );
    }
    add
}

fn add_menu(ui: &mut Ui, state: &State<'_>, commands: &mut Vec<Command>, anchor: Rect) {
    let open = ui.popup_open(ui.id("objects.add"));
    let mut chosen = None;
    grana_ui::popover::popover(ui, "objects.add", open, anchor, 196.0, |ui| {
        if grana_ui::popover::action_row(ui, "add.spots", grana_ui::Icon::Target, "Spots", None) {
            chosen = Some(ObjectKind::Spots);
        }
        if grana_ui::popover::action_row(ui, "add.surfaces", grana_ui::Icon::Cube, "Surfaces", None)
        {
            chosen = Some(ObjectKind::Surfaces);
        }
        if grana_ui::popover::action_row(
            ui,
            "add.filaments",
            grana_ui::Icon::Channel,
            "Filaments",
            None,
        ) {
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

fn empty(ui: &mut Ui, channel_name: &str) {
    let look = ui.theme().look(Role::Card, grana_ui::State::Rest);
    ui.column(
        Block::column()
            .padding(Edges::symmetric(16.0, 18.0))
            .gaps(8.0)
            .background(look),
        |ui| {
            let art = ui.allocate_full(42.0);
            let art_color = ui.theme().palette.text_tertiary;
            {
                let mut painter = ui.painter();
                painter.rect(
                    Rect::from_center_size(
                        Vec2::new(art.center().x, art.center().y),
                        Vec2::splat(38.0),
                    ),
                    Corners::capsule(),
                    Color::white_alpha(0.05),
                );
                grana_ui::icon::paint(
                    grana_ui::Icon::Layers,
                    &mut painter,
                    art.center(),
                    21.0,
                    art_color,
                    1.5,
                );
            }
            let title = ui.allocate_full(18.0);
            let title_style = TextStyle::new(
                ui.theme().font(grana_ui::Level::Headline),
                ui.theme().palette.text_primary,
            )
            .aligned(Align::Center);
            {
                let mut painter = ui.painter();
                painter.text_in(title, "No object sets", title_style, Align::Center);
            }
            let detail = format!(
                "Add spots, surfaces or filaments to detect and measure structures in {channel_name}."
            );
            let style = TextStyle::new(
                ui.theme().font(grana_ui::Level::Caption),
                ui.theme().palette.text_tertiary,
            )
            .wrapping()
            .aligned(Align::Center);
            ui.text_style_wrapped(&detail, style, ui.available().x);
        },
    );
}

fn kind_icon(kind: ObjectKind) -> grana_ui::Icon {
    match kind {
        ObjectKind::Spots => grana_ui::Icon::Target,
        ObjectKind::Surfaces => grana_ui::Icon::Cube,
        ObjectKind::Filaments => grana_ui::Icon::Channel,
    }
}

fn object(ui: &mut Ui, state: &mut State<'_>, commands: &mut Vec<Command>, id: ObjectSetId) {
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
    let open_id = format!("object.{}.open", id.raw());
    let expanded = ui.disclosure_open(&open_id, false);
    let look = state_card(ui);
    ui.push_id(&format!("object.{}", id.raw()));
    ui.column(
        Block::column()
            .padding(Edges::all(11.0))
            .gaps(8.0)
            .background(look),
        |ui| {
            let header = ui.allocate_full(44.0);
            let icon = Rect::from_center_size(
                Vec2::new(header.min.x + 14.0, header.center().y),
                Vec2::splat(27.0),
            );
            let menu_rect = Rect::from_center_size(
                Vec2::new(header.max.x - 12.0, header.center().y),
                Vec2::splat(24.0),
            );
            let eye_rect = Rect::from_center_size(
                Vec2::new(menu_rect.min.x - 23.0, header.center().y),
                Vec2::splat(24.0),
            );
            let response = ui.interact(
                ui.id("header"),
                Rect::from_min_max(header.min, Vec2::new(eye_rect.min.x - 6.0, header.max.y)),
            );
            if response.clicked {
                ui.store_open(&open_id, !expanded);
            }
            let arrow = ui.animate(
                ui.id("arrow"),
                if expanded { 1.0 } else { 0.0 },
                grana_ui::Motion::snappy(),
            );
            let tint = Color::from_rgb_f32(color);
            let chevron = ui.theme().palette.text_tertiary;
            let count = current.map(|(count, _level)| format!("{count} {}", kind.count_noun()));
            let subtitle = match count {
                Some(count) => {
                    if frames > 1 {
                        format!("{channel_name} · {count} · {detected} frames")
                    } else {
                        format!("{channel_name} · {count}")
                    }
                }
                None if detected > 0 => {
                    format!("{channel_name} · no objects on frame {}", time + 1)
                }
                None => format!("{channel_name} · not detected"),
            };
            let name_style = TextStyle::new(
                ui.theme().font(grana_ui::Level::Headline),
                ui.theme().palette.text_primary,
            )
            .truncating();
            let subtitle_style = TextStyle::new(
                grana_ui::TypeStyle::new(10.5, 400.0, 13.0),
                ui.theme().palette.text_tertiary,
            )
            .truncating();
            {
                let mut painter = ui.painter();
                painter.outlined(
                    icon,
                    Corners::all(8.0),
                    grana_ui::Fill::Solid(tint.with_alpha(0.16)),
                    grana_ui::Border {
                        width: 0.5,
                        color: tint.with_alpha(0.34),
                    },
                    grana_ui::Shadow::NONE,
                );
                grana_ui::icon::paint(
                    kind_icon(kind),
                    &mut painter,
                    icon.center(),
                    15.0,
                    tint,
                    1.6,
                );
                let text_area = Rect::from_min_max(
                    Vec2::new(icon.max.x + 9.0, header.min.y),
                    Vec2::new(eye_rect.min.x - 20.0, header.max.y),
                );
                painter.text_in(
                    Rect::from_min_max(
                        text_area.min,
                        Vec2::new(text_area.max.x, text_area.center().y + 3.0),
                    ),
                    &name,
                    name_style,
                    Align::Center,
                );
                painter.text_in(
                    Rect::from_min_max(
                        Vec2::new(text_area.min.x, text_area.center().y + 1.0),
                        text_area.max,
                    ),
                    &subtitle,
                    subtitle_style,
                    Align::Center,
                );
                let angle = std::f32::consts::FRAC_PI_2 * (0.5 + 0.5 * arrow);
                let centre = Vec2::new(eye_rect.min.x - 11.0, header.center().y);
                let tip = centre + Vec2::new(angle.cos(), angle.sin()) * 2.4;
                painter.segment(tip + Vec2::new(-2.2, -1.4), tip, 1.4, chevron);
                painter.segment(tip, tip + Vec2::new(2.2, -1.4), 1.4, chevron);
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
                    if visible {
                        grana_ui::Icon::Eye
                    } else {
                        grana_ui::Icon::EyeOff
                    },
                    24.0,
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
                    grana_ui::Icon::Ellipsis,
                    24.0,
                    Role::Quiet,
                    "Object commands",
                ) {
                    ui.toggle_popup(menu_key);
                }
                let open = ui.popup_open(menu_key);
                let mut remove = false;
                grana_ui::popover::popover(ui, "object.menu", open, menu_rect, 188.0, |ui| {
                    if grana_ui::popover::action_row(
                        ui,
                        "remove",
                        grana_ui::Icon::Trash,
                        "Remove object",
                        None,
                    ) {
                        remove = true;
                    }
                });
                if remove {
                    ui.close_popup();
                    commands.push(Command::RemoveObject(id));
                }
            });
            if expanded {
                divider(ui);
                widget::caption(ui, &format!("source channel {channel_name}"));
                params(ui, state, commands, id);
                detect(ui, state, commands, id, kind, frames);
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
                appearance(ui, state, commands, id);
                measure(ui, state, commands, id, time);
                if kind.trackable() {
                    crate::track_panel::entry(ui, state, id);
                }
                ui.row(Block::row().gaps(6.0).align(Align::Center), |ui| {
                    if detected > 0 && widget::button(ui, "clear", "Clear", Role::Danger) {
                        commands.push(Command::ClearObject(id));
                    }
                    ui.space(1.0);
                    widget::weak(ui, &format!("#{}", id.raw()));
                });
            }
        },
    );
    ui.pop_id();
    if let Some(set) = state.scene.objects.get(id) {
        if set.visible() != visible {
            commands.push(Command::SetObjectVisible { set: id, visible });
        }
        if set.color() != color {
            commands.push(Command::SetObjectColor { set: id, color });
        }
    }
}

fn divider(ui: &mut Ui) {
    let rect = ui.allocate_full(9.0);
    let color = ui.theme().palette.separator;
    let mut painter = ui.painter();
    painter.rect(
        Rect::from_min_size(
            Vec2::new(rect.min.x, rect.center().y),
            Vec2::new(rect.width(), 0.5),
        ),
        Corners::ZERO,
        color,
    );
}

fn state_card(ui: &mut Ui) -> grana_ui::Look {
    let mut look = ui.theme().look(Role::Card, grana_ui::State::Rest);
    look.corners = Corners::all(ui.theme().metrics.card_radius);
    look
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
    if changed {
        commands.push(Command::SetObjectParams { set: id, params });
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
    commands: &mut Vec<Command>,
    id: ObjectSetId,
    kind: ObjectKind,
    frames: u32,
) {
    let running = progress(state.jobs, Job::Detect(id));
    if running.is_none() {
        if widget::button_with(ui, "detect", "Detect", Role::Prominent, Length::Fill).clicked {
            commands.push(Command::Detect(id));
        }
    } else {
        ui.row(Block::row().gaps(8.0).align(Align::Center), |ui| {
            widget::spinner(ui, 14.0);
            if let Some(running) = running {
                caption(
                    ui,
                    &crate::panel::progress_caption(kind, Job::Detect(id), running, frames),
                );
                let fraction = if running.total > 0 {
                    running.done as f32 / running.total as f32
                } else {
                    0.0
                };
                let width = (ui.available().x - 74.0).max(60.0);
                widget::progress(ui, fraction, width);
            }
            if widget::button(ui, "cancel", "Cancel", Role::Quiet) {
                commands.push(Command::CancelDetect(id));
            }
        });
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
    ) {
        commands.push(Command::SetObjectColoring { set: id, coloring });
    }
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
            object_list(ui, state, commands, id, &entries, "surfaces", |volume| {
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
            object_list(ui, state, commands, id, &entries, "filaments", |length| {
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
    commands: &mut Vec<Command>,
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
        commands.push(Command::Select(Some(Selection {
            set: id,
            item: Item::Object(index),
        })));
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
