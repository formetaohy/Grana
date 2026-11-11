use super::*;

pub(super) fn origin<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
    id: ObjectSetId,
) -> Option<Element<'a, Message>> {
    let set = state.scene.objects.get(id)?;
    let language = ctx.language();
    let name = |id: ObjectSetId| {
        state.scene.objects.get(id).map_or_else(
            || grana_i18n::object::object_set(language).to_string(),
            |set| set.name().to_string(),
        )
    };
    let text = match set.provenance() {
        Provenance::Detection => return None,
        Provenance::Segmented { params } => grana_i18n::object::segmented(
            language,
            params.model().label(),
            params.model().license().name(),
        ),
        Provenance::Imported { label } => grana_i18n::object::imported_from(language, label),
        Provenance::Interop { file } => grana_i18n::object::imported_from(language, file),
        Provenance::Attached { file, .. } => grana_i18n::object::attached_from(language, file),
        Provenance::Filtered { source, .. } => {
            grana_i18n::object::filtered_from(language, &name(*source))
        }
        Provenance::Copied { source } => grana_i18n::object::copy_of(language, &name(*source)),
        Provenance::Edited { source, edits } => {
            grana_i18n::object::edited_from(language, &name(*source), edits.len())
        }
        Provenance::Masked { source, strokes } => {
            grana_i18n::object::painted_from(language, &name(*source), strokes.len())
        }
        Provenance::Merged { parts } => grana_i18n::object::merged_from(
            language,
            &parts
                .iter()
                .map(|part| part.name())
                .collect::<Vec<_>>()
                .join(" + "),
        ),
    };
    Some(view::labelled(
        ctx,
        grana_i18n::object::origin(language),
        &text,
    ))
}

pub(super) fn attached<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
    id: ObjectSetId,
) -> Option<Element<'a, Message>> {
    let set = state.scene.objects.get(id)?;
    let intensities = set.provenance().intensities()?;
    let language = ctx.language();
    let frames = (0..state.scene.frame_count())
        .filter(|time| set.frame(*time).is_some())
        .count();
    let objects = (0..state.scene.frame_count())
        .filter_map(|time| set.frame(time))
        .map(ObjectFrame::object_count)
        .sum::<usize>();
    let text = match intensities {
        IntensitySource::Statistics => {
            grana_i18n::object::import_statistics(language, frames, objects)
        }
        IntensitySource::Sampled => grana_i18n::object::import_sampled(language, frames, objects),
    };
    Some(view::labelled(
        ctx,
        grana_i18n::object::import(language),
        &text,
    ))
}

pub(super) fn curated<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
    id: ObjectSetId,
) -> Option<Element<'a, Message>> {
    let set = state.scene.objects.get(id)?;
    let curation = set.curation();
    if curation.is_empty() && curation.dropped() == 0 {
        return None;
    }
    let language = ctx.language();
    let mut parts = Vec::new();
    let removed = curation.removal_count();
    if removed > 0 {
        parts.push(grana_i18n::object::removed(language, removed));
    }
    let edits = curation.edit_count();
    if edits == 1 {
        parts.push(grana_i18n::object::track_edit(language, edits));
    } else if edits > 0 {
        parts.push(grana_i18n::object::track_edits(language, edits));
    }
    let pending = curation.pending();
    if pending > 0 {
        parts.push(grana_i18n::object::pending(language, pending));
    }
    let dropped = curation.dropped();
    if dropped > 0 {
        parts.push(grana_i18n::object::dropped(language, dropped));
    }
    Some(view::labelled(
        ctx,
        grana_i18n::object::curated(language),
        &parts.join(" · "),
    ))
}

pub(super) fn params<'a>(
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
    let language = ctx.language();
    let mut children = Vec::new();
    if let Some(segment) = set.provenance().segment() {
        let models = grana_scene::SegmentModel::available()
            .map(|model| {
                (
                    model,
                    format!("{} · {}", model.label(), model.license().name()),
                )
            })
            .collect::<Vec<_>>();
        let selected = segment.clone();
        if models.is_empty() {
            children.push(crate::view::caption(
                ctx,
                grana_i18n::object::no_segmentation_weights(language),
            ));
        } else if models.iter().any(|(model, _)| *model == segment.model()) {
            children.push(crate::inspector::form_combo(
                ctx,
                &format!("object.{}.segment.model", id.raw()),
                grana_i18n::object::model(language),
                &models,
                Some(segment.model()),
                220.0,
                move |model| {
                    Message::Command(Command::SetSegment {
                        set: id,
                        params: selected.clone().with_model(model),
                    })
                },
            ));
        } else {
            children.push(crate::view::labelled(
                ctx,
                grana_i18n::object::model(language),
                &grana_i18n::object::model_weights(
                    language,
                    segment.model().label(),
                    segment.model().license().name(),
                ),
            ));
            children.push(crate::view::caption(
                ctx,
                grana_i18n::object::no_model_weights(language),
            ));
        }
        for (index, field) in SEGMENT_FIELDS.iter().enumerate() {
            let (minimum, maximum) = bounds(field, voxel, world);
            let value = segment.field(index).clamp(minimum, maximum);
            let readout = format!("{value:.3}{}", field.suffix);
            let label = capitalized(&grana_i18n::object::parameter(language, field.label));
            let segment = segment.clone();
            children.push(crate::view::slider_readout(
                ctx,
                &format!("object.{}.segment.{index}", id.raw()),
                Some(&label),
                value,
                minimum..=maximum,
                field.logarithmic,
                &readout,
                move |value: f32| {
                    let mut next = segment.clone();
                    next.set_field(index, value.clamp(minimum, maximum));
                    Command::SetSegment {
                        set: id,
                        params: next,
                    }
                },
            ));
        }
        return children;
    }
    for (index, field) in kind.fields().iter().enumerate() {
        let (minimum, maximum) = bounds(field, voxel, world);
        let value = params.value(index).clamp(minimum, maximum);
        let readout = format!("{value:.3}{}", field.suffix);
        let label = capitalized(&grana_i18n::object::parameter(language, field.label));
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

pub(super) fn capitalized(label: &str) -> String {
    let mut characters = label.chars();
    match characters.next() {
        Some(first) => first.to_uppercase().collect::<String>() + characters.as_str(),
        None => String::new(),
    }
}

pub(super) fn bounds(field: &ParamField, voxel: f32, world: f32) -> (f32, f32) {
    (
        scale(field.minimum, voxel, world),
        scale(field.maximum, voxel, world),
    )
}

pub(super) fn scale(scale: ParamScale, voxel: f32, world: f32) -> f32 {
    match scale {
        ParamScale::Voxel(factor) => factor * voxel,
        ParamScale::World(fraction) => fraction * world,
        ParamScale::Absolute(value) => value,
    }
}

pub(super) fn detect<'a>(
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
    let language = ctx.language();
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
            let caption = view::caption(
                ctx,
                &view::progress_caption(language, kind, job, running, frames),
            );
            let progress = grana_ui::widget::progress(fraction, width());
            let cancel = view::button_with(
                ctx,
                grana_i18n::common::cancel(language),
                Role::Control,
                Length::Fill,
            )
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
                grana_i18n::object::detect(language).to_string()
            } else {
                grana_i18n::object::detect_root(language, &root_name)
            };
            view::button_with(ctx, &label, Role::Prominent, Length::Fill)
                .press(Message::Command(Command::Detect(id)))
        }
    }
}

pub(super) fn importing<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
    id: ObjectSetId,
    kind: ObjectKind,
) -> Vec<Element<'a, Message>> {
    let Some(running) = view::progress(state.jobs, Job::Import(id)) else {
        return Vec::new();
    };
    let language = ctx.language();
    let fraction = if running.total > 0 {
        running.done as f32 / running.total as f32
    } else {
        0.0
    };
    let spinner = view::spinner(ctx, 13.0);
    let caption = view::caption(
        ctx,
        &view::progress_caption(
            language,
            kind,
            Job::Import(id),
            running,
            state.scene.frame_count(),
        ),
    );
    let progress = grana_ui::widget::progress(fraction, width());
    let cancel = view::button_with(
        ctx,
        grana_i18n::common::cancel(language),
        Role::Control,
        Length::Fill,
    )
    .press(Message::Command(Command::CancelDetect(id)));
    vec![
        grana_ui::row()
            .gap(6.0)
            .align(Align::Center)
            .children([spinner, caption]),
        progress,
        cancel,
    ]
}

pub(super) fn seeds<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
    id: ObjectSetId,
) -> Vec<Element<'a, Message>> {
    let theme = *ctx.theme();
    let language = ctx.language();
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
    let mut options = vec![(
        0usize,
        grana_i18n::object::manual_seeds(language).to_string(),
    )];
    options.extend(
        candidates
            .iter()
            .enumerate()
            .map(|(index, (_, name))| (index + 1, grana_i18n::object::from_source(language, name))),
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
        grana_i18n::object::seeds(language),
        &options,
        selected.or(Some(0)),
        width() - crate::panel::FORM_LABEL,
        choose,
    ));
    if let Some(source) = source {
        let name = candidates
            .iter()
            .find(|(candidate, _)| *candidate == source)
            .map_or_else(
                || grana_i18n::object::seed_source(language),
                |(_, name)| name.as_str(),
            );
        children.push(view::labelled(
            ctx,
            grana_i18n::object::active_seeds(language),
            &grana_i18n::object::active_seeds_detail(language, count, name),
        ));
        return children;
    }
    let armed = state.workspace.seed == Some(id);
    let control = theme.metrics.control_height;
    let placed = crate::inspector::centered(
        control,
        view::caption(ctx, &grana_i18n::object::placed(language, count)),
    );
    let place = view::button(
        ctx,
        if armed {
            grana_i18n::object::stop_placing(language)
        } else {
            grana_i18n::object::place_seeds(language)
        },
        if armed { Role::Tinted } else { Role::Control },
    )
    .press(Message::SetSeed((!armed).then_some(id)));
    let mut cell = vec![placed, place];
    if count > 0 {
        cell.push(crate::inspector::centered(control, clear_seeds(ctx, id)));
    }
    let row = grana_ui::row().gap(6.0).align(Align::Center).children(cell);
    children.push(view::form_row(
        ctx,
        grana_i18n::object::manual(language),
        row,
    ));
    if armed {
        children.push(view::weak(ctx, grana_i18n::object::seed_hint(language)));
    }
    children
}

pub(super) fn clear_seeds<'a>(
    ctx: &mut BuildContext<'_, Message>,
    id: ObjectSetId,
) -> Element<'a, Message> {
    let language = ctx.language();
    view::icon_button(
        ctx,
        &format!("object.{}.seed.clear", id.raw()),
        Icon::Trash,
        22.0,
        Role::Quiet,
        grana_i18n::object::clear_seeds(language),
        Message::Command(Command::ClearSeeds(id)),
    )
}

pub(super) fn summary<'a>(
    ctx: &mut BuildContext<'_, Message>,
    kind: ObjectKind,
    current: Option<(usize, usize)>,
    mean: Option<f32>,
    detected: usize,
    frames: u32,
    time: u32,
) -> Element<'a, Message> {
    let language = ctx.language();
    match current {
        Some((count, level)) => {
            let noun = count_noun(language, kind);
            let mut text = grana_i18n::object::summary_count(language, count, noun, level + 1);
            if let Some(mean) = mean {
                let contrast = capitalized(
                    &grana_i18n::measure::term(language, "contrast")
                        .unwrap_or_else(|| "contrast".to_string()),
                );
                text.push_str(&grana_i18n::object::summary_contrast(
                    language, &contrast, mean,
                ));
            }
            view::caption(ctx, &text)
        }
        None if detected == 0 => view::weak(
            ctx,
            &grana_i18n::object::detect_summary(language, description(language, kind)),
        ),
        None => view::weak(
            ctx,
            &grana_i18n::object::no_objects_frame(language, time + 1, frames),
        ),
    }
}
