use super::*;

pub(super) fn region<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
    index: usize,
    count: usize,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let language = ctx.language();
    let current = state.scene.region;
    let size = current.size();
    let summary = if current.enabled() {
        format!("{:.1}×{:.1}×{:.1} µm", size.x, size.y, size.z)
    } else {
        grana_i18n::common::whole_volume(language).to_string()
    };
    let key = Key::new("region");
    let open = state.workspace.disclosures.is_open(key);
    let mut children = Vec::new();
    if open {
        let world = state.scene.dataset.world_size();
        let half = 0.5 * world;
        let minimum = (0.001 * world.min_element()).max(1.0e-4);
        let reach = world.length().max(minimum);
        children.push(
            grana_ui::widget::toggle_row(
                &theme,
                grana_i18n::inspector::limit_analysis(language),
                current.enabled(),
            )
            .named("region.enabled")
            .on_toggle(move |value| {
                let mut region = current;
                region.set_enabled(value);
                Message::Command(Command::SetRegion(region))
            }),
        );
        if current.enabled() {
            children.push(view::weak(
                ctx,
                grana_i18n::inspector::region_crops_export(language),
            ));
        }
        for (axis_idx, axis) in ["X", "Y", "Z"].into_iter().enumerate() {
            let value = current.center()[axis_idx];
            let readout = format!("{value:.2} µm");
            let label = grana_i18n::inspector::center(language, axis);
            children.push(
                grana_ui::widget::slider_readout(
                    &theme,
                    Some(&label),
                    value,
                    -half[axis_idx]..=half[axis_idx],
                    false,
                    &readout,
                )
                .named(&format!("region.center.{axis_idx}"))
                .on_slide(move |value| {
                    let mut region = current;
                    let mut center = current.center();
                    center[axis_idx] = value.clamp(-half[axis_idx], half[axis_idx]);
                    region.set_center(center);
                    Message::Command(Command::SetRegion(region))
                }),
            );
        }
        for (axis_idx, axis) in ["X", "Y", "Z"].into_iter().enumerate() {
            let value = size[axis_idx];
            let readout = format!("{value:.2} µm");
            let label = grana_i18n::inspector::size(language, axis);
            children.push(
                grana_ui::widget::slider_readout(
                    &theme,
                    Some(&label),
                    value,
                    minimum..=2.0 * reach,
                    false,
                    &readout,
                )
                .named(&format!("region.size.{axis_idx}"))
                .on_slide(move |value| {
                    let mut region = current;
                    let mut extents = current.half();
                    extents[axis_idx] = 0.5 * value.clamp(minimum, 2.0 * reach);
                    region.set_half(extents);
                    Message::Command(Command::SetRegion(region))
                }),
            );
        }
        let align = {
            let axes = [Vec3::X, Vec3::Y, Vec3::Z];
            let region = grana_volume::Region::new(current.center(), axes, current.half());
            Message::Command(Command::SetRegion(region))
        };
        let fill = Message::Command(Command::SetRegion(grana_volume::Region::full(-half, half)));
        let inner_sm = theme.metrics.corner_sm;
        let buttons = grana_ui::row().gap(theme.metrics.group_gap).children([
            view::button_shaped(
                ctx,
                grana_i18n::inspector::align_to_axes(language),
                Role::SurfaceHigh,
                Length::Fill,
                Corners::split_leading(inner_sm),
            )
            .press(align),
            view::button_shaped(
                ctx,
                grana_i18n::inspector::fill_volume(language),
                Role::SurfaceHigh,
                Length::Fill,
                Corners::split_trailing(inner_sm),
            )
            .press(fill),
        ]);
        children.push(buttons);
    }
    view::disclosure_group_card(
        ctx,
        (index, count),
        key,
        (
            Icon::Fit,
            grana_i18n::inspector::region_of_interest(language),
            Some(&summary),
        ),
        children,
        open,
    )
}

pub(super) fn drift<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
    index: usize,
    count: usize,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let language = ctx.language();
    let channel = state.scene.selected;
    let drift = state.scene.drift(channel);
    let running = view::progress(state.jobs, grana_scene::Job::Drift);
    let summary = if running.is_some() {
        grana_i18n::inspector::aligning(language).to_string()
    } else {
        match drift {
            None => grana_i18n::inspector::inactive(language).to_string(),
            Some(drift) => grana_i18n::inspector::drift_frames(language, drift.frames()),
        }
    };
    let key = Key::new("drift");
    let open = state.workspace.disclosures.is_open(key);
    let mut children = Vec::new();
    if open {
        match running {
            Some(progress) => {
                let text = if progress.total > 0 {
                    grana_i18n::inspector::aligning_progress(
                        language,
                        progress.done,
                        progress.total,
                    )
                } else {
                    grana_i18n::inspector::aligning_frames(language).to_string()
                };
                let fraction = if progress.total > 0 {
                    progress.done as f32 / progress.total as f32
                } else {
                    0.5
                };
                let spinner = view::spinner(ctx, 14.0);
                let caption = view::caption(ctx, &text);
                children.push(
                    grana_ui::row()
                        .gap(8.0)
                        .align(Align::Center)
                        .children([spinner, caption]),
                );
                children.push(grana_ui::widget::progress(fraction, card_width()));
                children.push(
                    view::button_with(
                        ctx,
                        grana_i18n::common::cancel(language),
                        Role::SurfaceHigh,
                        Length::Fill,
                    )
                    .press(Message::Command(Command::CancelDrift)),
                );
            }
            None => {
                children.push(
                    view::button_with(
                        ctx,
                        grana_i18n::inspector::correct_drift(language),
                        Role::PrimaryContainer,
                        Length::Fill,
                    )
                    .press(Message::Command(Command::ComputeDrift)),
                );
                let name = state.scene.dataset.channels()[channel].name().to_string();
                match drift {
                    None => children.push(view::weak(
                        ctx,
                        &grana_i18n::inspector::aligns_channel(language, &name),
                    )),
                    Some(drift) => {
                        let frames = drift.frames();
                        let reference = drift.reference();
                        let time = state.scene.time();
                        children.push(view::caption(
                            ctx,
                            &grana_i18n::inspector::drift_summary(
                                language,
                                frames,
                                drift.magnitude(),
                            ),
                        ));
                        if drift.deformed() {
                            let deformation = (0..frames)
                                .map(|frame| drift.deformation(frame))
                                .fold(0.0, f32::max);
                            children.push(view::weak(
                                ctx,
                                &grana_i18n::inspector::drift_deformation(language, deformation),
                            ));
                        }
                        let reference_field =
                            grana_ui::widget::drag_value(&theme, reference as f32, 0)
                                .named("drift.reference")
                                .on_drag_value(0.2, move |value| {
                                    Message::Command(Command::SetDriftReference(
                                        value.round().clamp(0.0, (frames - 1) as f32) as u32,
                                    ))
                                });
                        let unit = view::caption(ctx, grana_i18n::inspector::frame_unit(language));
                        let row = grana_ui::row()
                            .gap(6.0)
                            .align(Align::Center)
                            .children([reference_field, unit]);
                        children.push(view::form_row(
                            ctx,
                            grana_i18n::inspector::reference(language),
                            row,
                        ));
                        children.push(frame_table(ctx, time, drift));
                        children.push(
                            view::button_with(
                                ctx,
                                grana_i18n::inspector::clear_drift(language),
                                Role::Error,
                                Length::Fill,
                            )
                            .press(Message::Command(Command::ClearDrift)),
                        );
                    }
                }
            }
        }
    }
    view::disclosure_group_card(
        ctx,
        (index, count),
        key,
        (
            Icon::Target,
            grana_i18n::inspector::drift_correction(language),
            Some(&summary),
        ),
        children,
        open,
    )
}

pub(super) fn registration<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
    index: usize,
    count: usize,
) -> Element<'a, Message> {
    let language = ctx.language();
    let registration = state.scene.registration();
    let running = view::progress(state.jobs, grana_scene::Job::Register);
    let channels = state.scene.base_channels();
    let summary = if running.is_some() {
        grana_i18n::inspector::registering(language).to_string()
    } else if registration.is_empty() {
        grana_i18n::inspector::inactive(language).to_string()
    } else {
        grana_i18n::inspector::registration_summary(
            language,
            registration.channels(),
            registration.magnitude(),
        )
    };
    let key = Key::new("registration");
    let open = state.workspace.disclosures.is_open(key);
    let mut children = Vec::new();
    if open {
        match running {
            Some(progress) => {
                let text = if progress.total > 0 {
                    grana_i18n::inspector::registering_channels(
                        language,
                        progress.done,
                        progress.total,
                    )
                } else {
                    grana_i18n::inspector::registering(language).to_string()
                };
                let fraction = if progress.total > 0 {
                    progress.done as f32 / progress.total as f32
                } else {
                    0.5
                };
                let spinner = view::spinner(ctx, 14.0);
                let caption = view::caption(ctx, &text);
                children.push(
                    grana_ui::row()
                        .gap(8.0)
                        .align(Align::Center)
                        .children([spinner, caption]),
                );
                children.push(grana_ui::widget::progress(fraction, card_width()));
                children.push(
                    view::button_with(
                        ctx,
                        grana_i18n::common::cancel(language),
                        Role::SurfaceHigh,
                        Length::Fill,
                    )
                    .press(Message::Command(Command::CancelRegistration)),
                );
            }
            None => {
                let reference = state.scene.selected.min(channels.saturating_sub(1));
                children.push(
                    view::button_with(
                        ctx,
                        grana_i18n::inspector::register_channels(language),
                        Role::PrimaryContainer,
                        Length::Fill,
                    )
                    .press(Message::Command(Command::RegisterChannels(reference))),
                );
                if fiducial(state, reference, channels) {
                    children.push(
                        view::button_with(
                            ctx,
                            grana_i18n::inspector::from_localizations(language),
                            Role::SurfaceHigh,
                            Length::Fill,
                        )
                        .press(Message::Command(
                            Command::RegisterFromLocalizations(reference),
                        )),
                    );
                }
                if registration.is_empty() {
                    let name = state.scene.dataset.channels()[reference].name().to_string();
                    children.push(view::weak(
                        ctx,
                        &grana_i18n::inspector::registration_hint(language, &name),
                    ));
                } else {
                    children.push(view::caption(
                        ctx,
                        &grana_i18n::inspector::registration_summary(
                            language,
                            registration.channels(),
                            registration.magnitude(),
                        ),
                    ));
                    if registration.deformed() {
                        children.push(view::weak(
                            ctx,
                            &grana_i18n::inspector::registration_deformation(
                                language,
                                registration.deformation(),
                            ),
                        ));
                    }
                    children.push(
                        view::button_with(
                            ctx,
                            grana_i18n::inspector::clear_registration(language),
                            Role::Error,
                            Length::Fill,
                        )
                        .press(Message::Command(Command::ClearRegistration)),
                    );
                }
            }
        }
    }
    view::disclosure_group_card(
        ctx,
        (index, count),
        key,
        (
            Icon::Layers,
            grana_i18n::inspector::channel_registration(language),
            Some(&summary),
        ),
        children,
        open,
    )
}

fn fiducial(state: &View<'_>, reference: usize, channels: usize) -> bool {
    let mut source = false;
    let mut moving = false;
    for set in state.scene.objects.iter() {
        if set.kind() != grana_scene::ObjectKind::Spots
            || set.channel() >= channels
            || set.localization().is_none()
        {
            continue;
        }
        if set.channel() == reference {
            source = true;
        } else {
            moving = true;
        }
    }
    source && moving
}

pub(super) fn frame_table<'a>(
    ctx: &mut BuildContext<'_, Message>,
    time: u32,
    drift: &grana_scene::Drift,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let language = ctx.language();
    let reference = drift.reference();
    let mut rows = Vec::new();
    for frame in 0..drift.frames() {
        let offset = drift.translation(frame);
        let mut label = grana_i18n::inspector::frame_number(language, frame + 1);
        if frame == time {
            label.push_str(" •");
        }
        if frame == reference {
            label.push_str(grana_i18n::inspector::reference_suffix(language));
        }
        rows.push(view::labelled(
            ctx,
            &label,
            &format!("{:+0.2} {:+0.2} {:+0.2}", offset[0], offset[1], offset[2]),
        ));
    }
    let list = grana_ui::column().gap(0.0).children(rows);
    let mut children = vec![
        Element::new(grana_ui::Layout::column())
            .named("drift.frames")
            .scroll(grana_ui::Axis::Column, Length::Fixed(116.0), None)
            .clipped()
            .children([list]),
    ];
    if time == reference {
        children.push(view::weak(
            ctx,
            grana_i18n::inspector::reference_fixed(language),
        ));
    } else {
        let offset = drift.translation(time);
        for (axis, name) in ["X", "Y", "Z"].into_iter().enumerate() {
            let field = grana_ui::widget::drag_value(&theme, offset[axis], 2)
                .named(&format!("drift.offset.{axis}"))
                .on_drag_value(0.05, move |value| {
                    let mut next = offset;
                    next[axis] = (value * 100.0).round() / 100.0;
                    Message::Command(Command::SetDriftOffset { time, offset: next })
                });
            let unit = view::caption(ctx, "µm");
            let row = grana_ui::row()
                .gap(6.0)
                .align(Align::Center)
                .children([field, unit]);
            children.push(view::form_row(
                ctx,
                &grana_i18n::inspector::offset_axis(language, name),
                row,
            ));
        }
    }
    grana_ui::column().gap(6.0).children(children)
}

pub(super) fn mask<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
    index: usize,
    count: usize,
) -> Element<'a, Message> {
    let language = ctx.language();
    let sources = state
        .scene
        .objects
        .iter()
        .map(|set| (set.id(), set.name().to_string(), set.filtered()))
        .collect::<Vec<_>>();
    let current = state.scene.mask;
    let summary = current
        .and_then(|id| sources.iter().find(|(set, _, _)| *set == id))
        .map_or_else(
            || grana_i18n::common::whole_volume(language).to_string(),
            |(_, name, _)| name.clone(),
        );
    let key = Key::new("mask.card");
    let open = state.workspace.disclosures.is_open(key);
    let mut children = Vec::new();
    if open {
        let mut options = vec![(
            None,
            grana_i18n::common::whole_volume_title(language).to_string(),
        )];
        options.extend(
            sources
                .iter()
                .map(|(id, name, _)| (Some(*id), name.clone())),
        );
        children.push(form_combo(
            ctx,
            "mask",
            grana_i18n::inspector::source(language),
            &options,
            Some(current),
            card_width() - crate::panel::FORM_LABEL,
            move |chosen| {
                if chosen == current {
                    Message::Noop
                } else {
                    Message::Command(Command::SetMask(chosen))
                }
            },
        ));
        let limit = state.scene.dataset.world_size().min_element().max(1.0);
        let radius = state.scene.mask_radius;
        let readout = format!("{radius:.2} µm");
        children.push(view::slider_readout(
            ctx,
            "mask.radius",
            Some(grana_i18n::inspector::grow_by(language)),
            radius,
            (0.0..=limit, false),
            &readout,
            Command::SetMaskRadius,
        ));
        if let Some(source) = current {
            let relations = state
                .scene
                .objects
                .get(source)
                .map(|set| {
                    set.relations()
                        .iter()
                        .filter_map(|relation| {
                            let target = state.scene.objects.get(relation.target())?;
                            let label = if relation.shift() == 0 {
                                target.name().to_string()
                            } else {
                                format!("{} · t{:+}", target.name(), relation.shift())
                            };
                            Some((Some((relation.target(), relation.shift())), label))
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            if !relations.is_empty() {
                let mut options = vec![(
                    None,
                    grana_i18n::inspector::any_relation(language).to_string(),
                )];
                options.extend(relations);
                let selected = state
                    .scene
                    .mask_filter
                    .map(|filter| (filter.target(), filter.shift()));
                let current_filter = state.scene.mask_filter;
                let world = state.scene.dataset.world_size();
                let width = card_width() - crate::panel::FORM_LABEL;
                children.push(form_combo(
                    ctx,
                    "mask.relation",
                    grana_i18n::inspector::relation(language),
                    &options,
                    Some(selected),
                    width,
                    move |chosen| {
                        let Some((target, shift)) = chosen else {
                            return Message::Command(Command::SetMaskFilter(None));
                        };
                        let (field, range) = match current_filter {
                            Some(filter) => (filter.field(), filter.range()),
                            None => (
                                RelationField::Distance,
                                default_filter_range(RelationField::Distance, world),
                            ),
                        };
                        Message::Command(Command::SetMaskFilter(Some(MaskFilter::new(
                            target, shift, field, range[0], range[1],
                        ))))
                    },
                ));
                if let Some(filter) = current_filter {
                    let theme = *ctx.theme();
                    let field = filter.field();
                    let target = filter.target();
                    let shift = filter.shift();
                    let range = filter.range();
                    let fields = RelationField::CONTINUOUS
                        .into_iter()
                        .map(|field| (field, relation_field(language, field)))
                        .collect::<Vec<_>>();
                    children.push(form_combo(
                        ctx,
                        "mask.field",
                        grana_i18n::inspector::field(language),
                        &fields,
                        Some(field),
                        width,
                        move |chosen| {
                            let range = if chosen == field {
                                range
                            } else {
                                default_filter_range(chosen, world)
                            };
                            Message::Command(Command::SetMaskFilter(Some(MaskFilter::new(
                                target, shift, chosen, range[0], range[1],
                            ))))
                        },
                    ));
                    let suffix = field.unit().suffix();
                    let bounds = [
                        (grana_i18n::common::minimum(language), range[0]),
                        (grana_i18n::common::maximum(language), range[1]),
                    ];
                    for (bound_idx, (label, value)) in bounds.into_iter().enumerate() {
                        let other = match bound_idx {
                            0 => range[1],
                            _ => range[0],
                        };
                        let action = move |value: f32| {
                            let (minimum, maximum) = if bound_idx == 0 {
                                (value.min(other), other)
                            } else {
                                (other, value.max(other))
                            };
                            Message::Command(Command::SetMaskFilter(Some(MaskFilter::new(
                                target, shift, field, minimum, maximum,
                            ))))
                        };
                        let widget =
                            grana_ui::widget::drag_value(&theme, value, field.decimals() as usize)
                                .named(&format!("mask.filter.{bound_idx}"))
                                .on_drag_value(unit_step(field), action);
                        let unit = view::caption(ctx, suffix);
                        let row = grana_ui::row()
                            .gap(6.0)
                            .align(Align::Center)
                            .children([widget, unit]);
                        children.push(view::form_row(ctx, label, row));
                    }
                    children.push(view::weak(
                        ctx,
                        &grana_i18n::inspector::detects_relation(
                            language,
                            &relation_field(language, field),
                            range[0],
                            range[1],
                            suffix,
                        ),
                    ));
                    let target_name = state.scene.objects.get(target).map_or_else(
                        || grana_i18n::inspector::relation_target(language).to_string(),
                        |set| set.name().to_string(),
                    );
                    children.push(view::weak(
                        ctx,
                        &grana_i18n::inspector::target_without_mask(language, &target_name),
                    ));
                }
            }
        }
        let detail = match current.and_then(|id| sources.iter().find(|(set, _, _)| *set == id)) {
            Some((_, name, true)) => grana_i18n::inspector::detects_filtered(language, name),
            Some((_, name, false)) => grana_i18n::inspector::detects_inside(language, name),
            None => grana_i18n::inspector::detects_all(language).to_string(),
        };
        children.push(view::weak(ctx, &detail));
    }
    view::disclosure_group_card(
        ctx,
        (index, count),
        key,
        (
            Icon::Filter,
            grana_i18n::inspector::analysis_mask(language),
            Some(&summary),
        ),
        children,
        open,
    )
}

fn relation_field(language: grana_i18n::Language, field: RelationField) -> String {
    grana_i18n::measure::term(language, field.label()).unwrap_or_else(|| field.label().to_string())
}

pub(super) fn default_filter_range(field: RelationField, world: Vec3) -> [f32; 2] {
    let reach = (0.5 * world.length()).max(1.0);
    match field {
        RelationField::Distance | RelationField::Center => [0.0, reach],
        RelationField::Inside => [0.0, 100.0],
        RelationField::Contact => [0.0, reach * reach],
        RelationField::Partner | RelationField::Overlapping => [0.0, 0.0],
    }
}

pub(super) fn unit_step(field: RelationField) -> f32 {
    match field {
        RelationField::Distance | RelationField::Center => 0.1,
        RelationField::Inside
        | RelationField::Contact
        | RelationField::Partner
        | RelationField::Overlapping => 1.0,
    }
}
