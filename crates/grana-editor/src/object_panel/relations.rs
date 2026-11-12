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
    let language = ctx.language();
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
        grana_i18n::object::relations(language),
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
    let language = ctx.language();
    let name = targets
        .iter()
        .find(|(candidate, _)| *candidate == target)
        .map_or(grana_i18n::object::object_set(language), |(_, name)| {
            name.as_str()
        });
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
        24.0,
        Role::SurfaceHigh,
        grana_i18n::object::open_pair_matrix(language),
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
        24.0,
        Role::Standard,
        grana_i18n::object::remove_relation(language),
        Message::Command(Command::RemoveRelation {
            source: id,
            target,
            shift,
        }),
    );
    let mut children = vec![grana_ui::row().gap(6.0).align(Align::Center).children([
        crate::inspector::centered(24.0, caption),
        crate::inspector::centered(24.0, count),
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
                22.0,
                Role::Standard,
                grana_i18n::object::earlier_frame(language),
                Message::Command(Command::MoveRelation {
                    source: id,
                    target,
                    shift,
                    next: shift - 1,
                }),
            ));
        }
        cell.push(crate::inspector::centered(
            22.0,
            view::caption(ctx, &shift_label(language, shift)),
        ));
        if shift < limit && !taken.contains(&(shift + 1)) {
            cell.push(view::icon_button(
                ctx,
                &format!("relation.{}.{}.{}.forward", id.raw(), target.raw(), shift),
                Icon::ChevronRight,
                22.0,
                Role::Standard,
                grana_i18n::object::later_frame(language),
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
                22.0,
                Role::SurfaceHigh,
                grana_i18n::object::add_offset_relation(language),
                Message::Noop,
            ));
        }
        let row = grana_ui::row().gap(4.0).align(Align::Center).children(cell);
        children.push(view::form_row(
            ctx,
            grana_i18n::object::shift(language),
            row,
        ));
        if !free.is_empty() {
            let mut rows = Vec::new();
            for (index, offset) in free.iter().enumerate() {
                rows.push(view::menu_row(
                    ctx,
                    &format!("{offset_key}.{index}"),
                    &shift_label(language, *offset),
                    false,
                    Some(Message::Command(Command::AddRelation {
                        source: id,
                        target,
                        shift: *offset,
                    })),
                ));
            }
            children.push(view::menu(ctx, Key::new(&offset_key), 180.0, rows));
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

pub(super) fn shift_label(language: Language, shift: i32) -> String {
    match shift {
        0 => grana_i18n::object::same_frame(language).to_string(),
        1 => grana_i18n::object::target_shift(language, "+1"),
        -1 => grana_i18n::object::target_shift(language, "−1"),
        value if value > 0 => grana_i18n::object::target_shift(language, &format!("+{value}")),
        value => grana_i18n::object::target_shift(language, &format!("{value}")),
    }
}

pub(super) fn unlinked<'a>(
    ctx: &mut BuildContext<'_, Message>,
    id: ObjectSetId,
    relations: &[RelationEntry],
    targets: &[(ObjectSetId, String)],
) -> Vec<Element<'a, Message>> {
    let language = ctx.language();
    let unlinked = targets
        .iter()
        .filter(|(target, _)| {
            !relations
                .iter()
                .any(|candidate| candidate.target == *target)
        })
        .map(|(target, name)| {
            let label = if *target == id {
                grana_i18n::object::nearest(language, name)
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
    let anchor = view::button_with(
        ctx,
        grana_i18n::object::relate_to_set(language),
        Role::SurfaceHigh,
        Length::Fill,
    )
    .named(&key)
    .press(Message::Noop);
    let menu = view::menu(ctx, Key::new(&key), 212.0, rows);
    vec![anchor, menu]
}
