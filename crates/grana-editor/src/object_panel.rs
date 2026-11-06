use glam::Vec2;
use grana_scene::{
    Item, Job, ObjectColor, ObjectFrame, ObjectKind, ObjectSet, ObjectSetId, ParamField,
    ParamScale, Provenance, RelationField, Scene,
};
use grana_session::Command;
use grana_ui::{
    Align, Border, BuildContext, Color, Corners, Edges, Element, Fill, Icon, Key, Length, Level,
    Motion, Rect, Role, Shadow, TextStyle, TypeStyle, icon,
};

use crate::message::Message;
use crate::state::View;
use crate::view;

const HEADER: f32 = 36.0;
const PAD: f32 = 11.0;

fn section_font() -> TypeStyle {
    TypeStyle {
        size: 10.0,
        weight: 600.0,
        tracking: 0.7,
        leading: 14.0,
    }
}

fn width() -> f32 {
    crate::inspector::panel_width() - PAD * 2.0
}

#[derive(Clone, Copy)]
struct RelationEntry {
    target: ObjectSetId,
    shift: i32,
    computed: usize,
    total: usize,
}

pub fn view<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
) -> Vec<Element<'a, Message>> {
    let channel = state.scene.selected;
    let channel_name = state.scene.channel(channel).name().to_string();
    let count = state.scene.objects.len();
    let mut elements = add_menu(ctx, state);
    elements.insert(0, header(ctx, count));
    if count == 0 {
        elements.push(view::empty_state(
            ctx,
            Icon::Layers,
            "No Object Sets",
            &format!(
                "Detect spots, surfaces or filaments in {channel_name} to measure structures."
            ),
        ));
        return elements;
    }
    let ids = state
        .scene
        .objects
        .iter()
        .map(ObjectSet::id)
        .collect::<Vec<_>>();
    let sets = ids.len();
    for (position, id) in ids.into_iter().enumerate() {
        elements.push(object(ctx, state, id, position, sets));
    }
    elements
}

fn header<'a>(ctx: &mut BuildContext<'_, Message>, count: usize) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let palette = theme.palette;
    let total = crate::inspector::panel_width();
    let label_style = TextStyle::new(section_font(), palette.text_tertiary);
    let detail_style =
        TextStyle::new(theme.font(Level::Caption), palette.text_tertiary).aligned(Align::End);
    let detail = count.to_string();
    let canvas = grana_ui::canvas(Length::Fixed(total - 24.0), 26.0, move |painter, info| {
        let rect = info.rect;
        painter.text_in(rect, "OBJECT SETS", label_style, Align::Center);
        painter.text_in(
            Rect::from_min_max(rect.min, Vec2::new(rect.max.x - 8.0, rect.max.y)),
            &detail,
            detail_style,
            Align::Center,
        );
    });
    let add = view::icon_button(
        ctx,
        "objects.add",
        Icon::Plus,
        24.0,
        Role::Quiet,
        "Add object set",
        Message::Noop,
    );
    grana_ui::row()
        .width(Length::Fill)
        .height(Length::Fixed(26.0))
        .align(Align::Center)
        .children([canvas, add])
}

fn add_menu<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
) -> Vec<Element<'a, Message>> {
    let loading = state.io.loading.is_some();
    let mut rows = Vec::new();
    for (key, symbol, label, kind) in [
        ("add.spots", Icon::Target, "Spots", ObjectKind::Spots),
        ("add.surfaces", Icon::Cube, "Surfaces", ObjectKind::Surfaces),
        (
            "add.filaments",
            Icon::Channel,
            "Filaments",
            ObjectKind::Filaments,
        ),
    ] {
        let message = if loading {
            Message::Noop
        } else {
            Message::Command(Command::AddObject(kind))
        };
        rows.push(action_row(ctx, key, symbol, label, message));
    }
    vec![view::menu(ctx, Key::new("objects.add"), 184.0, rows)]
}

fn action_row<'a>(
    ctx: &mut BuildContext<'_, Message>,
    key: &str,
    symbol: Icon,
    label: &str,
    message: Message,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    grana_ui::popover::action_row(&theme, symbol, label, None)
        .named(key)
        .press(message)
}

fn kind_icon(kind: ObjectKind) -> Icon {
    match kind {
        ObjectKind::Spots => Icon::Target,
        ObjectKind::Surfaces => Icon::Cube,
        ObjectKind::Filaments => Icon::Channel,
    }
}

fn object<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
    id: ObjectSetId,
    position: usize,
    sets: usize,
) -> Element<'a, Message> {
    let Some(set) = state.scene.objects.get(id) else {
        return crate::inspector::hidden();
    };
    let theme = *ctx.theme();
    let look = theme.surface(Role::Card, grana_ui::State::Rest);
    let width = width();
    let kind = set.kind();
    let merged = set.provenance().is_merged();
    let frames = state.scene.frame_count();
    let time = state.scene.time();
    let detected = set.detected();
    let current = set
        .frame(time)
        .map(|frame| (frame.object_count(), frame.level()));
    let mean = set
        .frame(time)
        .and_then(ObjectFrame::spots)
        .and_then(|spots| spots.mean_quality());
    let expanded = state.workspace.expanded(id);
    let mut children = Vec::new();
    children.push(header_row(ctx, state, id, position, sets));
    if expanded {
        children.push(view::divider(ctx));
        children.extend(appearance_fields(ctx, state, id, width));
        if let Some(element) = origin(ctx, state, id) {
            children.push(element);
        }
        if let Some(element) = curated(ctx, state, id) {
            children.push(element);
        }
        if !merged {
            children.extend(params(ctx, state, id));
            if kind == ObjectKind::Surfaces {
                children.extend(seeds(ctx, state, id));
            }
            children.push(detect(ctx, state, id, kind, frames));
        }
        children.push(summary(ctx, kind, current, mean, detected, frames, time));
        children.extend(appearance(ctx, state, id, width));
        children.extend(relations(ctx, state, id));
        if state
            .scene
            .objects
            .get(id)
            .is_some_and(|set| set.kind().trackable())
        {
            children.push(crate::track_panel::entry(ctx, state, id));
        }
        children.extend(measure(ctx, state, id, time));
        if detected > 0 {
            children.push(view::divider(ctx));
            children.push(
                view::button_with(ctx, "Clear Objects", Role::Danger, Length::Fill)
                    .press(Message::Command(Command::ClearObject(id))),
            );
        }
    }
    grana_ui::column()
        .padding(Edges::all(PAD))
        .gap(7.0)
        .background(look)
        .children(children)
}

fn appearance_fields<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
    id: ObjectSetId,
    width: f32,
) -> Vec<Element<'a, Message>> {
    let theme = *ctx.theme();
    let Some(set) = state.scene.objects.get(id) else {
        return Vec::new();
    };
    let raw = id.raw();
    let opacity = set.opacity();
    let labels = set.labels();
    let label_metric = set.label_metric();
    let hidden = set.hidden_count();
    let time = state.scene.time();
    let selected = state
        .scene
        .selection()
        .and_then(|selection| match selection.item {
            Item::Object(object) if selection.set == id && set.visible_object(time, object) => {
                Some(object)
            }
            _ => None,
        });
    let mut children = Vec::new();
    let readout = format!("{:.0}%", opacity * 100.0);
    children.push(view::slider_readout(
        ctx,
        &format!("object.{raw}.opacity"),
        Some("Opacity"),
        opacity,
        0.0..=1.0,
        false,
        &readout,
        move |value| Command::SetObjectOpacity {
            set: id,
            opacity: value.clamp(0.0, 1.0),
        },
    ));
    children.push(
        grana_ui::widget::toggle_row(&theme, "Object Labels", labels)
            .named(&format!("object.{raw}.labels"))
            .on_toggle(move |value| {
                Message::Command(Command::SetObjectLabels {
                    set: id,
                    labels: value,
                })
            }),
    );
    if labels {
        let options = label_metrics(state.scene, set);
        children.push(crate::inspector::form_combo(
            ctx,
            &format!("object.{raw}.label.metric"),
            "Label by",
            &options,
            Some(label_metric),
            width - crate::panel::FORM_LABEL,
            move |chosen: Option<ObjectColor>| {
                Message::Command(Command::SetLabelMetric {
                    set: id,
                    metric: chosen,
                })
            },
        ));
    }
    if hidden > 0 || selected.is_some() {
        let control = theme.metrics.control_height;
        let mut row = Vec::new();
        if hidden > 0 {
            row.push(crate::inspector::centered(
                control,
                view::caption(ctx, &format!("{hidden} hidden")),
            ));
            row.push(
                view::button(ctx, "Show All", Role::Quiet)
                    .press(Message::Command(Command::ClearHidden(id))),
            );
        }
        if let Some(object) = selected {
            row.push(
                view::button(ctx, "Hide Selected", Role::Quiet).press(Message::Command(
                    Command::SetObjectHidden {
                        set: id,
                        time,
                        objects: vec![object],
                        hidden: true,
                    },
                )),
            );
        }
        children.push(grana_ui::row().gap(6.0).align(Align::Center).children(row));
    }
    children
}

fn header_row<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
    id: ObjectSetId,
    position: usize,
    sets: usize,
) -> Element<'a, Message> {
    let Some(set) = state.scene.objects.get(id) else {
        return crate::inspector::hidden();
    };
    let theme = *ctx.theme();
    let palette = theme.palette;
    let raw = id.raw();
    let name = set.name().to_string();
    let kind = set.kind();
    let channel = set.channel();
    let channel_name = state.scene.dataset.channels()[channel].name().to_string();
    let tint = Color::from_rgb_f32(set.color());
    let visible = set.visible();
    let detected = set.detected();
    let hidden = set.hidden_count();
    let frames = state.scene.frame_count();
    let time = state.scene.time();
    let current = set
        .frame(time)
        .map(|frame| (frame.object_count(), frame.level()));
    let expanded = state.workspace.expanded(id);
    let width = width();
    let name_key = Key::new(format!("object.{raw}.name"));
    let editing = ctx.is_editing(name_key) || state.workspace.renaming == Some(id);
    let arrow = ctx.animate(
        Key::new(format!("object.{raw}.arrow")).id(),
        if expanded { 1.0 } else { 0.0 },
        Motion::snappy(),
    );
    let subtitle = match current {
        Some((count, _level)) => {
            if frames > 1 {
                format!(
                    "{channel_name} · {count} {} · {detected}f",
                    kind.count_noun()
                )
            } else {
                format!("{channel_name} · {count} {}", kind.count_noun())
            }
        }
        None if detected > 0 => {
            format!("{channel_name} · 0 on frame {}", time + 1)
        }
        None => format!("{channel_name} · Ready to detect"),
    };
    let subtitle = if hidden > 0 {
        format!("{subtitle} · {hidden} hidden")
    } else {
        subtitle
    };
    let name_style =
        TextStyle::new(theme.font(Level::Body).medium(), palette.text_primary).truncating();
    let subtitle_style =
        TextStyle::new(TypeStyle::new(10.5, 400.0, 13.0), palette.text_tertiary).truncating();
    let toggle = move || Message::ExpandObject {
        set: id,
        open: !expanded,
    };
    let body = grana_ui::canvas(Length::Fixed(40.0), HEADER, move |painter, info| {
        let rect = info.rect;
        let tile = Rect::from_center_size(
            Vec2::new(rect.min.x + 15.0, rect.center().y),
            Vec2::splat(30.0),
        );
        painter.rect(tile, Corners::all(9.0), tint.with_alpha(0.15));
        painter.outlined(
            tile,
            Corners::all(9.0),
            Fill::None,
            Border {
                width: 1.0,
                color: tint.with_alpha(0.32),
            },
            Shadow::NONE,
        );
        icon::paint(kind_icon(kind), painter, tile.center(), 14.0, tint, 1.5);
    })
    .key(Key::new(format!("object.{raw}.header.icon")))
    .press(toggle());
    let text_width = width - 114.0;
    let text = if editing {
        let current_name = name.clone();
        grana_ui::widget::text_field(&name, name_style, text_width, false)
            .key(name_key)
            .editing(true)
            .on_commit(move |text: &str| {
                let next = text.trim().to_string();
                if next == current_name {
                    Message::EndRename
                } else {
                    Message::RenameObject {
                        set: id,
                        name: next,
                    }
                }
            })
            .on_cancel(|| Message::EndRename)
    } else {
        let title = name.clone();
        grana_ui::canvas(Length::Fixed(text_width), HEADER, move |painter, info| {
            let rect = info.rect;
            painter.text_in(
                Rect::from_min_max(
                    Vec2::new(rect.min.x, rect.min.y + 1.0),
                    Vec2::new(rect.max.x, rect.center().y + 1.0),
                ),
                &title,
                name_style,
                Align::Center,
            );
            painter.text_in(
                Rect::from_min_max(Vec2::new(rect.min.x, rect.center().y + 2.0), rect.max),
                &subtitle,
                subtitle_style,
                Align::Center,
            );
        })
        .key(Key::new(format!("object.{raw}.header.text")))
        .press(toggle())
    };
    let eye = view::icon_button(
        ctx,
        &format!("object.{raw}.visible"),
        if visible { Icon::Eye } else { Icon::EyeOff },
        24.0,
        Role::Quiet,
        if visible {
            "Hide object set"
        } else {
            "Show object set"
        },
        Message::Command(Command::SetObjectVisible {
            set: id,
            visible: !visible,
        }),
    );
    let menu_button = view::icon_button(
        ctx,
        &format!("object.{raw}.menu"),
        Icon::Ellipsis,
        24.0,
        Role::Quiet,
        "Object commands",
        Message::Noop,
    );
    let chevron = grana_ui::canvas(Length::Fixed(16.0), 24.0, move |painter, info| {
        icon::chevron(
            painter,
            info.rect.center(),
            8.0,
            std::f32::consts::FRAC_PI_2 * arrow,
            palette.text_tertiary,
            1.5,
        );
    })
    .key(Key::new(format!("object.{raw}.arrow.anchor")))
    .press(toggle())
    .focus_ring(Corners::all(8.0))
    .cursor(grana_ui::Cursor::Pointing);
    let rows = object_menu(ctx, state, id, position, sets);
    let menu = view::menu(ctx, Key::new(format!("object.{raw}.menu")), 188.0, rows);
    let gap = grana_ui::canvas(Length::Fixed(6.0), 0.0, |_painter, _info| {});
    let thin = grana_ui::canvas(Length::Fixed(2.0), 0.0, |_painter, _info| {});
    let thin_two = grana_ui::canvas(Length::Fixed(2.0), 0.0, |_painter, _info| {});
    let row = grana_ui::row()
        .width(Length::Fixed(width))
        .height(Length::Fixed(HEADER))
        .align(Align::Center)
        .children([
            body,
            crate::inspector::centered(HEADER, text),
            gap,
            crate::inspector::centered(HEADER, eye),
            thin,
            crate::inspector::centered(HEADER, menu_button),
            thin_two,
            crate::inspector::centered(HEADER, chevron),
        ]);
    grana_ui::column().gap(0.0).children([row, menu])
}

fn object_menu<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
    id: ObjectSetId,
    position: usize,
    sets: usize,
) -> Vec<Element<'a, Message>> {
    let Some(set) = state.scene.objects.get(id) else {
        return Vec::new();
    };
    let kind = set.kind();
    let merges = state
        .scene
        .objects
        .iter()
        .filter(|candidate| candidate.id() != id && candidate.kind() == kind)
        .map(|candidate| (candidate.id(), candidate.name().to_string()))
        .collect::<Vec<_>>();
    let mut rows = Vec::new();
    rows.push(action_row(
        ctx,
        &format!("object.{}.menu.rename", id.raw()),
        Icon::Pencil,
        "Rename",
        Message::BeginRename(id),
    ));
    rows.push(action_row(
        ctx,
        &format!("object.{}.menu.duplicate", id.raw()),
        Icon::Layers,
        "Duplicate",
        Message::Command(Command::DuplicateObject(id)),
    ));
    if kind != ObjectKind::Spots {
        rows.push(action_row(
            ctx,
            &format!("object.{}.menu.export", id.raw()),
            Icon::Export,
            "Export Mesh…",
            Message::Command(Command::ExportMesh(id)),
        ));
    }
    rows.push(action_row(
        ctx,
        &format!("object.{}.menu.labels", id.raw()),
        Icon::Export,
        "Export Labels…",
        Message::Command(Command::ExportLabels(id)),
    ));
    if !merges.is_empty() {
        rows.push(view::menu_separator(ctx));
        rows.push(view::caption(ctx, "Merge Into"));
        for (index, (candidate, label)) in merges.iter().enumerate() {
            rows.push(view::menu_row(
                ctx,
                &format!("object.{}.merge.{index}", id.raw()),
                label,
                false,
                Some(Message::Command(Command::MergeObjects {
                    source: id,
                    target: *candidate,
                })),
            ));
        }
    }
    rows.push(view::menu_separator(ctx));
    if position > 0 {
        rows.push(action_row(
            ctx,
            &format!("object.{}.menu.up", id.raw()),
            Icon::ChevronUp,
            "Move Up",
            Message::Command(Command::ReorderObject {
                set: id,
                index: position - 1,
            }),
        ));
    }
    if position + 1 < sets {
        rows.push(action_row(
            ctx,
            &format!("object.{}.menu.down", id.raw()),
            Icon::ChevronDown,
            "Move Down",
            Message::Command(Command::ReorderObject {
                set: id,
                index: position + 1,
            }),
        ));
    }
    rows.push(action_row(
        ctx,
        &format!("object.{}.menu.remove", id.raw()),
        Icon::Trash,
        "Delete Object Set",
        Message::Command(Command::RemoveObject(id)),
    ));
    rows
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

fn origin<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
    id: ObjectSetId,
) -> Option<Element<'a, Message>> {
    let set = state.scene.objects.get(id)?;
    let name = |id: ObjectSetId| {
        state
            .scene
            .objects
            .get(id)
            .map_or("Object Set".to_string(), |set| set.name().to_string())
    };
    let text = match set.provenance() {
        Provenance::Detection => return None,
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
    Some(view::labelled(ctx, "Origin", &text))
}

fn curated<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
    id: ObjectSetId,
) -> Option<Element<'a, Message>> {
    let set = state.scene.objects.get(id)?;
    let curation = set.curation();
    if curation.is_empty() && curation.dropped() == 0 {
        return None;
    }
    let mut parts = Vec::new();
    let removed = curation.removal_count();
    if removed > 0 {
        parts.push(format!("{removed} removed"));
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
        parts.push(format!("{pending} pending"));
    }
    let dropped = curation.dropped();
    if dropped > 0 {
        parts.push(format!("{dropped} dropped"));
    }
    Some(view::labelled(ctx, "Curated", &parts.join(" · ")))
}

fn params<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
    id: ObjectSetId,
) -> Vec<Element<'a, Message>> {
    let Some(set) = state.scene.objects.get(id) else {
        return Vec::new();
    };
    let kind = set.kind();
    let voxel = state.scene.dataset.voxel_size().min_element();
    let world = state.scene.dataset.world_size().max_element();
    let params = set.params();
    let mut children = Vec::new();
    for (index, field) in kind.fields().iter().enumerate() {
        let (minimum, maximum) = bounds(field, voxel, world);
        let value = params.value(index).clamp(minimum, maximum);
        let readout = format!("{value:.3}{}", field.suffix);
        let label = capitalized(field.label);
        children.push(crate::view::slider_readout(
            ctx,
            &format!("object.{}.param.{index}", id.raw()),
            Some(&label),
            value,
            minimum..=maximum,
            field.logarithmic,
            &readout,
            move |value: f32| {
                let mut next = params;
                next.set_value(index, value.clamp(minimum, maximum));
                Command::SetObjectParams {
                    set: id,
                    params: next,
                }
            },
        ));
    }
    children
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

fn detect<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
    id: ObjectSetId,
    kind: ObjectKind,
    frames: u32,
) -> Element<'a, Message> {
    let root = state.scene.detection_root(id).unwrap_or(id);
    let root_name = state
        .scene
        .objects
        .get(root)
        .map_or_else(String::new, |set| set.name().to_string());
    let running = view::progress(state.jobs, Job::Detect(root))
        .map(|running| (Job::Detect(root), running))
        .or_else(|| {
            view::progress(state.jobs, Job::Recut(root)).map(|running| (Job::Recut(root), running))
        });
    match running {
        Some((job, running)) => {
            let fraction = if running.total > 0 {
                running.done as f32 / running.total as f32
            } else {
                0.0
            };
            let spinner = view::spinner(ctx, 13.0);
            let caption = view::caption(ctx, &view::progress_caption(kind, job, running, frames));
            let progress = grana_ui::widget::progress(fraction, width());
            let cancel = view::button_with(ctx, "Cancel", Role::Control, Length::Fill)
                .press(Message::Command(Command::CancelDetect(id)));
            grana_ui::column().gap(6.0).children([
                grana_ui::row()
                    .gap(6.0)
                    .align(Align::Center)
                    .children([spinner, caption]),
                progress,
                cancel,
            ])
        }
        None => {
            let label = if root == id {
                "Detect".to_string()
            } else {
                format!("Detect {root_name}")
            };
            view::button_with(ctx, &label, Role::Prominent, Length::Fill)
                .press(Message::Command(Command::Detect(id)))
        }
    }
}

fn seeds<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
    id: ObjectSetId,
) -> Vec<Element<'a, Message>> {
    let theme = *ctx.theme();
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
    let mut options = vec![(0usize, "Manual Seeds".to_string())];
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
    let mut children = Vec::new();
    let choose = {
        let candidates = candidates.clone();
        move |index: usize| {
            let chosen = (index > 0).then(|| candidates[index - 1].0);
            Message::Command(Command::SetSeedSource {
                set: id,
                source: chosen,
            })
        }
    };
    children.push(crate::inspector::form_combo(
        ctx,
        &format!("object.{}.seed.source", id.raw()),
        "Seeds",
        &options,
        selected.or(Some(0)),
        width() - crate::panel::FORM_LABEL,
        choose,
    ));
    if let Some(source) = source {
        let name = candidates
            .iter()
            .find(|(candidate, _)| *candidate == source)
            .map_or("seed source", |(_, name)| name.as_str());
        children.push(view::labelled(
            ctx,
            "Active seeds",
            &format!("{count} from {name}"),
        ));
        return children;
    }
    let armed = state.workspace.seed == Some(id);
    let control = theme.metrics.control_height;
    let placed =
        crate::inspector::centered(control, view::caption(ctx, &format!("{count} placed")));
    let place = view::button(
        ctx,
        if armed { "Stop Placing" } else { "Place Seeds" },
        if armed { Role::Tinted } else { Role::Control },
    )
    .press(Message::SetSeed((!armed).then_some(id)));
    let mut cell = vec![placed, place];
    if count > 0 {
        cell.push(crate::inspector::centered(control, clear_seeds(ctx, id)));
    }
    let row = grana_ui::row().gap(6.0).align(Align::Center).children(cell);
    children.push(view::form_row(ctx, "Manual", row));
    if armed {
        children.push(view::weak(
            ctx,
            "Click a surface to place a seed · Alt-click to remove.",
        ));
    }
    children
}

fn clear_seeds<'a>(ctx: &mut BuildContext<'_, Message>, id: ObjectSetId) -> Element<'a, Message> {
    view::icon_button(
        ctx,
        &format!("object.{}.seed.clear", id.raw()),
        Icon::Trash,
        22.0,
        Role::Quiet,
        "Clear seeds",
        Message::Command(Command::ClearSeeds(id)),
    )
}

fn summary<'a>(
    ctx: &mut BuildContext<'_, Message>,
    kind: ObjectKind,
    current: Option<(usize, usize)>,
    mean: Option<f32>,
    detected: usize,
    frames: u32,
    time: u32,
) -> Element<'a, Message> {
    match current {
        Some((count, level)) => {
            let mut text = format!("{count} {} · Level {}", kind.count_noun(), level + 1);
            if let Some(mean) = mean {
                text.push_str(&format!(" · Contrast {mean:.2}"));
            }
            view::caption(ctx, &text)
        }
        None if detected == 0 => {
            view::weak(ctx, &format!("{} across all frames.", kind.description()))
        }
        None => view::weak(
            ctx,
            &format!("No objects on frame {} of {frames}.", time + 1),
        ),
    }
}

fn appearance<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
    id: ObjectSetId,
    width: f32,
) -> Vec<Element<'a, Message>> {
    let Some(set) = state.scene.objects.get(id) else {
        return Vec::new();
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
                ObjectColor::Measurement(measurement),
                measurement.label(&names),
            )
        })
        .collect::<Vec<_>>();
    for relation in set.relations() {
        let target = state
            .scene
            .objects
            .get(relation.target())
            .map_or_else(|| "Target".to_string(), |target| target.name().to_string());
        let shift = match relation.shift() {
            0 => String::new(),
            shift => format!(" · t{shift:+}"),
        };
        for field in RelationField::CONTINUOUS {
            let metric = ObjectColor::Relation {
                target: relation.target(),
                field,
                shift: relation.shift(),
            };
            metrics.push((metric, format!("{target} · {}{shift}", field.label())));
        }
    }
    let extent = set.object_extent();
    let coloring = set.object_coloring().clone();
    let salt = format!("object.{}.coloring", id.raw());
    let key = Key::new(&salt);
    let open = state.workspace.sections.is_open(key);
    let mut children = Vec::new();
    if open {
        children.extend(crate::coloring::edit(
            ctx, &salt, id, &coloring, &metrics, extent, width,
        ));
    }
    vec![crate::inspector::disclosure(
        ctx, &salt, "Coloring", open, children,
    )]
}

fn relations<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
    id: ObjectSetId,
) -> Vec<Element<'a, Message>> {
    let Some(set) = state.scene.objects.get(id) else {
        return Vec::new();
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
        return Vec::new();
    }
    let relations = set
        .relations()
        .iter()
        .map(|relation| RelationEntry {
            target: relation.target(),
            shift: relation.shift(),
            computed: relation.computed(),
            total: relation.frame_count(),
        })
        .collect::<Vec<_>>();
    let salt = format!("object.{}.relations", id.raw());
    let key = Key::new(&salt);
    let open = state.workspace.sections.is_open(key);
    let mut children = Vec::new();
    if open {
        for entry in relations.iter().copied() {
            children.push(relation_row(
                ctx, state, id, entry, frames, &relations, &targets,
            ));
        }
        children.extend(unlinked(ctx, id, frames, &relations, &targets));
    }
    vec![crate::inspector::disclosure(
        ctx,
        &salt,
        "Relations",
        open,
        children,
    )]
}

fn relation_row<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
    id: ObjectSetId,
    entry: RelationEntry,
    frames: u32,
    relations: &[RelationEntry],
    targets: &[(ObjectSetId, String)],
) -> Element<'a, Message> {
    let RelationEntry {
        target,
        shift,
        computed,
        total,
    } = entry;
    let name = targets
        .iter()
        .find(|(candidate, _)| *candidate == target)
        .map_or("Object Set", |(_, name)| name.as_str());
    let running = view::progress(
        state.jobs,
        Job::Relation {
            source: id,
            target,
            shift,
        },
    );
    let (done, total) = match running {
        Some(running) => (running.done, running.total),
        None => (computed, total),
    };
    let mut label = name.to_string();
    if shift != 0 {
        label.push_str(&format!(" · t{shift:+}"));
    }
    let caption = view::caption(ctx, &label);
    let count = view::caption(ctx, &format!("{done}/{total}"));
    let remove = view::icon_button(
        ctx,
        &format!("relation.{}.{}.{}.remove", id.raw(), target.raw(), shift),
        Icon::Close,
        22.0,
        Role::Quiet,
        "Remove relation",
        Message::Command(Command::RemoveRelation {
            source: id,
            target,
            shift,
        }),
    );
    let mut children = vec![grana_ui::row().gap(6.0).align(Align::Center).children([
        crate::inspector::centered(22.0, caption),
        crate::inspector::centered(22.0, count),
        remove,
    ])];
    if frames > 1 {
        let limit = frames as i32 - 1;
        let taken = relations
            .iter()
            .filter(|candidate| candidate.target == target)
            .map(|candidate| candidate.shift)
            .collect::<Vec<_>>();
        let free = free_offsets(target, id, limit, &taken);
        let mut cell = Vec::new();
        if shift > -limit && !taken.contains(&(shift - 1)) {
            cell.push(view::icon_button(
                ctx,
                &format!("relation.{}.{}.{}.back", id.raw(), target.raw(), shift),
                Icon::ChevronLeft,
                20.0,
                Role::Quiet,
                "Earlier frame",
                Message::Command(Command::MoveRelation {
                    source: id,
                    target,
                    shift,
                    next: shift - 1,
                }),
            ));
        }
        cell.push(crate::inspector::centered(
            20.0,
            view::caption(ctx, &shift_label(shift)),
        ));
        if shift < limit && !taken.contains(&(shift + 1)) {
            cell.push(view::icon_button(
                ctx,
                &format!("relation.{}.{}.{}.forward", id.raw(), target.raw(), shift),
                Icon::ChevronRight,
                20.0,
                Role::Quiet,
                "Later frame",
                Message::Command(Command::MoveRelation {
                    source: id,
                    target,
                    shift,
                    next: shift + 1,
                }),
            ));
        }
        let offset_key = format!("relation.{}.{}.{}.offset", id.raw(), target.raw(), shift);
        if !free.is_empty() {
            cell.push(view::icon_button(
                ctx,
                &offset_key,
                Icon::Plus,
                20.0,
                Role::Quiet,
                "Add offset relation",
                Message::Noop,
            ));
        }
        let row = grana_ui::row().gap(4.0).align(Align::Center).children(cell);
        children.push(view::form_row(ctx, "Shift", row));
        if !free.is_empty() {
            let mut rows = Vec::new();
            for (index, offset) in free.iter().enumerate() {
                rows.push(view::menu_row(
                    ctx,
                    &format!("{offset_key}.{index}"),
                    &shift_label(*offset),
                    false,
                    Some(Message::Command(Command::AddRelation {
                        source: id,
                        target,
                        shift: *offset,
                    })),
                ));
            }
            children.push(view::menu(ctx, Key::new(&offset_key), 176.0, rows));
        }
    }
    grana_ui::column().gap(6.0).children(children)
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
        0 => "Same frame".to_string(),
        1 => "Target +1f".to_string(),
        -1 => "Target −1f".to_string(),
        value if value > 0 => format!("Target +{value}f"),
        value => format!("Target {value}f"),
    }
}

fn unlinked<'a>(
    ctx: &mut BuildContext<'_, Message>,
    id: ObjectSetId,
    frames: u32,
    relations: &[RelationEntry],
    targets: &[(ObjectSetId, String)],
) -> Vec<Element<'a, Message>> {
    let unlinked = targets
        .iter()
        .filter(|(target, _)| {
            !relations
                .iter()
                .any(|candidate| candidate.target == *target)
                && (*target != id || frames > 1)
        })
        .map(|(target, name)| {
            let label = if *target == id {
                format!("{name} · Earlier frames")
            } else {
                name.clone()
            };
            (*target, label)
        })
        .collect::<Vec<_>>();
    if unlinked.is_empty() {
        return Vec::new();
    }
    let key = format!("relation.add.{}", id.raw());
    let mut rows = Vec::new();
    for (index, (target, label)) in unlinked.iter().enumerate() {
        rows.push(view::menu_row(
            ctx,
            &format!("{key}.{index}"),
            label,
            false,
            Some(Message::Command(Command::AddRelation {
                source: id,
                target: *target,
                shift: if *target == id { -1 } else { 0 },
            })),
        ));
    }
    let anchor = view::button_with(ctx, "Relate to Object Set…", Role::Control, Length::Fill)
        .named(&key)
        .press(Message::Noop);
    let menu = view::menu(ctx, Key::new(&key), 208.0, rows);
    vec![anchor, menu]
}

fn measure<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
    id: ObjectSetId,
    time: u32,
) -> Vec<Element<'a, Message>> {
    let Some(index) = state.scene.selection().and_then(|selection| {
        (selection.set == id)
            .then_some(selection.item)
            .and_then(|item| match item {
                Item::Object(index) => Some(index),
                Item::Track(_) => None,
            })
    }) else {
        return Vec::new();
    };
    let Some(frame) = state
        .scene
        .objects
        .get(id)
        .and_then(|object| object.frame(time))
    else {
        return Vec::new();
    };
    let frame = frame.clone();
    let channel = state
        .scene
        .objects
        .get(id)
        .map_or(0, |object| object.channel());
    let mut children = vec![
        view::divider(ctx),
        view::section_header(ctx, "Selected Object", None),
    ];
    let rows = match &frame {
        ObjectFrame::Spots(spots) => {
            let Some(spot) = spots.spots().get(index).copied() else {
                return children;
            };
            let centre = spot.center();
            let intensity = frame.intensities().object(index, channel);
            vec![
                ("Object", format!("#{}", index + 1)),
                (
                    "Position",
                    format!("{:.2}, {:.2}, {:.2} µm", centre[0], centre[1], centre[2]),
                ),
                ("Diameter", format!("{:.3} µm", spot.radius() * 2.0)),
                ("Contrast", format!("{:.3}", spot.quality())),
                ("Intensity", intensity_text(intensity)),
            ]
        }
        ObjectFrame::Surfaces(surfaces) => {
            let Some(object) = surfaces.object(index) else {
                return children;
            };
            let intensity = surfaces.intensities().object(index, channel);
            let centroid = object.centroid();
            let size = object.bounds().size();
            vec![
                ("Object", format!("#{}", index + 1)),
                ("Volume", format!("{:.2} µm³", object.volume())),
                ("Area", format!("{:.2} µm²", object.area())),
                ("Sphericity", format!("{:.3}", object.sphericity())),
                (
                    "Centroid",
                    format!(
                        "{:.2}, {:.2}, {:.2} µm",
                        centroid[0], centroid[1], centroid[2]
                    ),
                ),
                (
                    "Extent",
                    format!("{:.2} × {:.2} × {:.2} µm", size[0], size[1], size[2]),
                ),
                ("Voxels", object.voxel_count().to_string()),
                ("Intensity", intensity_text(intensity)),
            ]
        }
        ObjectFrame::Filaments(filaments) => {
            let Some(object) = filaments.object(index) else {
                return children;
            };
            let intensity = filaments.intensities().object(index, channel);
            let centroid = object.centroid();
            let size = object.bounds().size();
            vec![
                ("Object", format!("#{}", index + 1)),
                ("Length", format!("{:.2} µm", object.length())),
                ("Diameter", format!("{:.2} µm", object.diameter())),
                ("Volume", format!("{:.2} µm³", object.volume())),
                ("Branches", object.branches().to_string()),
                ("Terminals", object.terminals().to_string()),
                ("Junctions", object.junctions().to_string()),
                ("Nodes", object.nodes().to_string()),
                (
                    "Centroid",
                    format!(
                        "{:.2}, {:.2}, {:.2} µm",
                        centroid[0], centroid[1], centroid[2]
                    ),
                ),
                (
                    "Extent",
                    format!("{:.2} × {:.2} × {:.2} µm", size[0], size[1], size[2]),
                ),
                ("Intensity", intensity_text(intensity)),
            ]
        }
    };
    for (label, value) in rows {
        children.push(view::labelled(ctx, label, &value));
    }
    children.push(
        view::button_with(ctx, "Delete Object", Role::Danger, Length::Fill).press(
            Message::Command(Command::DeleteObject {
                set: id,
                time,
                object: index,
            }),
        ),
    );
    children
}

fn intensity_text(intensity: grana_object::SampleStatistics) -> String {
    format!(
        "{:.0} ± {:.0} ({:.0}–{:.0})",
        intensity.mean(),
        intensity.deviation(),
        intensity.minimum(),
        intensity.maximum(),
    )
}
