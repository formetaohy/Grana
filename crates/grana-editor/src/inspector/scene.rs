use super::*;

pub(super) fn region<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let current = state.scene.region;
    let size = current.size();
    let summary = if current.enabled() {
        format!("{:.1}×{:.1}×{:.1} µm", size.x, size.y, size.z)
    } else {
        "Whole volume".to_string()
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
            grana_ui::widget::toggle_row(&theme, "Limit Analysis", current.enabled())
                .named("region.enabled")
                .on_toggle(move |value| {
                    let mut region = current;
                    region.set_enabled(value);
                    Message::Command(Command::SetRegion(region))
                }),
        );
        for (name, index) in [("Center X", 0), ("Center Y", 1), ("Center Z", 2)] {
            let value = current.center()[index];
            let readout = format!("{value:.2} µm");
            children.push(
                grana_ui::widget::slider_readout(
                    &theme,
                    Some(name),
                    value,
                    -half[index]..=half[index],
                    false,
                    &readout,
                )
                .named(&format!("region.center.{index}"))
                .on_slide(move |value| {
                    let mut region = current;
                    let mut center = current.center();
                    center[index] = value.clamp(-half[index], half[index]);
                    region.set_center(center);
                    Message::Command(Command::SetRegion(region))
                }),
            );
        }
        for (name, index) in [("Size X", 0), ("Size Y", 1), ("Size Z", 2)] {
            let value = size[index];
            let readout = format!("{value:.2} µm");
            children.push(
                grana_ui::widget::slider_readout(
                    &theme,
                    Some(name),
                    value,
                    minimum..=2.0 * reach,
                    false,
                    &readout,
                )
                .named(&format!("region.size.{index}"))
                .on_slide(move |value| {
                    let mut region = current;
                    let mut extents = current.half();
                    extents[index] = 0.5 * value.clamp(minimum, 2.0 * reach);
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
        let buttons = grana_ui::row().gap(8.0).children([
            view::button_with(ctx, "Align to Axes", Role::Control, Length::Fill).press(align),
            view::button_with(ctx, "Fill Volume", Role::Control, Length::Fill).press(fill),
        ]);
        children.push(buttons);
    }
    view::disclosure_card(
        ctx,
        key,
        Icon::Fit,
        "Region of Interest",
        Some(&summary),
        children,
        open,
    )
}

pub(super) fn drift<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let drift = state.scene.drift();
    let running = view::progress(state.jobs, grana_scene::Job::Drift);
    let summary = if running.is_some() {
        "Aligning…".to_string()
    } else if drift.is_empty() {
        "Off".to_string()
    } else {
        format!("{} frames", drift.frames())
    };
    let key = Key::new("drift");
    let open = state.workspace.disclosures.is_open(key);
    let mut children = Vec::new();
    if open {
        match running {
            Some(progress) => {
                let text = if progress.total > 0 {
                    format!("Aligning · {}/{} frames", progress.done, progress.total)
                } else {
                    "Aligning frames…".to_string()
                };
                let spinner = view::spinner(ctx, 12.0);
                let caption = view::caption(ctx, &text);
                children.push(
                    grana_ui::row()
                        .gap(8.0)
                        .align(Align::Center)
                        .children([spinner, caption]),
                );
                children.push(
                    view::button_with(ctx, "Cancel", Role::Control, Length::Fill)
                        .press(Message::Command(Command::CancelDrift)),
                );
            }
            None => {
                children.push(
                    view::button_with(ctx, "Correct Drift", Role::Control, Length::Fill)
                        .press(Message::Command(Command::ComputeDrift)),
                );
                if drift.is_empty() {
                    let channel = state.scene.dataset.channels()[state.scene.selected]
                        .name()
                        .to_string();
                    children.push(view::weak(
                        ctx,
                        &format!("Aligns {channel} across all time frames."),
                    ));
                } else {
                    let frames = drift.frames();
                    let reference = drift.reference();
                    let time = state.scene.time();
                    children.push(view::caption(
                        ctx,
                        &format!(
                            "{frames} frames aligned · max {:.1} voxels",
                            drift.magnitude()
                        ),
                    ));
                    let reference_field = grana_ui::widget::drag_value(&theme, reference as f32, 0)
                        .named("drift.reference")
                        .on_drag_value(0.2, move |value| {
                            Message::Command(Command::SetDriftReference(
                                value.round().clamp(0.0, (frames - 1) as f32) as u32,
                            ))
                        });
                    let unit = view::caption(ctx, "frame");
                    let row = grana_ui::row()
                        .gap(6.0)
                        .align(Align::Center)
                        .children([reference_field, unit]);
                    children.push(view::form_row(ctx, "Reference", row));
                    children.push(frame_table(ctx, state, time));
                    children.push(
                        view::button_with(ctx, "Clear Drift", Role::Danger, Length::Fill)
                            .press(Message::Command(Command::ClearDrift)),
                    );
                }
            }
        }
    }
    view::disclosure_card(
        ctx,
        key,
        Icon::Target,
        "Drift Correction",
        Some(&summary),
        children,
        open,
    )
}

pub(super) fn frame_table<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
    time: u32,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let drift = state.scene.drift();
    let reference = drift.reference();
    let mut rows = Vec::new();
    for frame in 0..drift.frames() {
        let offset = drift.offset(frame);
        let mut label = format!("Frame {}", frame + 1);
        if frame == time {
            label.push_str(" •");
        }
        if frame == reference {
            label.push_str(" (ref)");
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
        children.push(view::weak(ctx, "Reference frame is fixed at zero offset."));
    } else {
        let offset = drift.offset(time);
        for (axis, name) in ["Offset X", "Offset Y", "Offset Z"].into_iter().enumerate() {
            let field = grana_ui::widget::drag_value(&theme, offset[axis], 2)
                .named(&format!("drift.offset.{axis}"))
                .on_drag_value(0.5, move |value| {
                    let mut next = offset;
                    next[axis] = (value * 100.0).round() / 100.0;
                    Message::Command(Command::SetDriftOffset { time, offset: next })
                });
            let unit = view::caption(ctx, "vx");
            let row = grana_ui::row()
                .gap(6.0)
                .align(Align::Center)
                .children([field, unit]);
            children.push(view::form_row(ctx, name, row));
        }
    }
    grana_ui::column().gap(6.0).children(children)
}

pub(super) fn mask<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
) -> Element<'a, Message> {
    let sources = state
        .scene
        .objects
        .iter()
        .map(|set| (set.id(), set.name().to_string(), set.filtered()))
        .collect::<Vec<_>>();
    let current = state.scene.mask;
    let summary = current
        .and_then(|id| sources.iter().find(|(set, _, _)| *set == id))
        .map_or_else(|| "Whole volume".to_string(), |(_, name, _)| name.clone());
    let key = Key::new("mask.card");
    let open = state.workspace.disclosures.is_open(key);
    let mut children = Vec::new();
    if open {
        let mut options = vec![(None, "Whole Volume".to_string())];
        options.extend(
            sources
                .iter()
                .map(|(id, name, _)| (Some(*id), name.clone())),
        );
        children.push(form_combo(
            ctx,
            "mask",
            "Source",
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
            Some("Grow by"),
            radius,
            0.0..=limit,
            false,
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
                let mut options = vec![(None, "Any Relation".to_string())];
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
                    "Relation",
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
                        .map(|field| (field, field.label().to_string()))
                        .collect::<Vec<_>>();
                    children.push(form_combo(
                        ctx,
                        "mask.field",
                        "Field",
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
                    for (index, (label, value)) in [("Minimum", range[0]), ("Maximum", range[1])]
                        .into_iter()
                        .enumerate()
                    {
                        let other = match index {
                            0 => range[1],
                            _ => range[0],
                        };
                        let action = move |value: f32| {
                            let (minimum, maximum) = if index == 0 {
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
                                .named(&format!("mask.filter.{index}"))
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
                        &format!(
                            "Detects only where the mask relation keeps objects whose {} lies in {:.2}{suffix} to {:.2}{suffix}.",
                            field.label(),
                            range[0],
                            range[1]
                        ),
                    ));
                    let target_name = state.scene.objects.get(target).map_or_else(
                        || "the relation target".to_string(),
                        |set| set.name().to_string(),
                    );
                    children.push(view::weak(
                        ctx,
                        &format!(
                            "{target_name} is detected without the mask so its relation can limit the mask."
                        ),
                    ));
                }
            }
        }
        children.push(
            match current.and_then(|id| sources.iter().find(|(set, _, _)| *set == id)) {
                Some((_, name, true)) => {
                    view::weak(ctx, &format!("Detects inside filtered objects of {name}."))
                }
                Some((_, name, false)) => {
                    view::weak(ctx, &format!("Detects inside {name} objects only."))
                }
                None => view::weak(ctx, "Detects across the entire dataset."),
            },
        );
    }
    view::disclosure_card(
        ctx,
        key,
        Icon::Filter,
        "Analysis Mask",
        Some(&summary),
        children,
        open,
    )
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
