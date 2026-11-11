use super::*;

#[derive(Clone, Copy)]
pub(super) struct RelationEntry {
    target: ObjectSetId,
    shift: i32,
    computed: usize,
    total: usize,
}

pub(super) fn relations<'a>(
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
    let open = state.workspace.disclosures.is_open(key);
    let mut children = Vec::new();
    if open {
        for entry in relations.iter().copied() {
            children.push(relation_row(
                ctx, state, id, entry, frames, &relations, &targets,
            ));
        }
        children.extend(unlinked(ctx, id, &relations, &targets));
    }
    vec![crate::inspector::disclosure(
        ctx,
        &salt,
        "Relations",
        open,
        children,
    )]
}

pub(super) fn relation_row<'a>(
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
    let matrix = view::icon_button(
        ctx,
        &format!("relation.{}.{}.{}.matrix", id.raw(), target.raw(), shift),
        Icon::Grid,
        22.0,
        Role::Quiet,
        "Open pair matrix",
        Message::OpenRelation {
            source: id,
            target,
            shift,
        },
    );
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
        matrix,
        remove,
    ])];
    if frames > 1 {
        let limit = frames as i32 - 1;
        let taken = relations
            .iter()
            .filter(|candidate| candidate.target == target)
            .map(|candidate| candidate.shift)
            .collect::<Vec<_>>();
        let free = free_offsets(limit, &taken);
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

pub(super) fn free_offsets(limit: i32, taken: &[i32]) -> Vec<i32> {
    let mut free = (-limit..=limit)
        .filter(|offset| !taken.contains(offset))
        .collect::<Vec<_>>();
    free.sort_by_key(|offset| (offset.abs(), *offset));
    free
}

pub(super) fn shift_label(shift: i32) -> String {
    match shift {
        0 => "Same frame".to_string(),
        1 => "Target +1f".to_string(),
        -1 => "Target −1f".to_string(),
        value if value > 0 => format!("Target +{value}f"),
        value => format!("Target {value}f"),
    }
}

pub(super) fn unlinked<'a>(
    ctx: &mut BuildContext<'_, Message>,
    id: ObjectSetId,
    relations: &[RelationEntry],
    targets: &[(ObjectSetId, String)],
) -> Vec<Element<'a, Message>> {
    let unlinked = targets
        .iter()
        .filter(|(target, _)| {
            !relations
                .iter()
                .any(|candidate| candidate.target == *target)
        })
        .map(|(target, name)| {
            let label = if *target == id {
                format!("{name} · Nearest Neighbour")
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
                shift: 0,
            })),
        ));
    }
    let anchor = view::button_with(ctx, "Relate to Object Set…", Role::Control, Length::Fill)
        .named(&key)
        .press(Message::Noop);
    let menu = view::menu(ctx, Key::new(&key), 208.0, rows);
    vec![anchor, menu]
}
