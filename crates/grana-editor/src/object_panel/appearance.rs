use super::*;

pub(super) fn appearance_fields<'a>(
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
    let language = ctx.language();
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
        Some(grana_i18n::object::opacity(language)),
        opacity,
        0.0..=1.0,
        false,
        &readout,
        move |value| Command::SetObjectOpacity {
            set: id,
            opacity: value.clamp(0.0, 1.0),
        },
    ));
    let material = set.material();
    let roughness = format!("{:.2}", material.roughness());
    children.push(view::slider_readout(
        ctx,
        &format!("object.{raw}.roughness"),
        Some(grana_i18n::object::roughness(language)),
        material.roughness(),
        0.0..=1.0,
        false,
        &roughness,
        move |value| Command::SetObjectMaterial {
            set: id,
            material: material.with_roughness(value.clamp(0.0, 1.0)),
        },
    ));
    let metallic = format!("{:.2}", material.metallic());
    children.push(view::slider_readout(
        ctx,
        &format!("object.{raw}.metallic"),
        Some(grana_i18n::object::metallic(language)),
        material.metallic(),
        0.0..=1.0,
        false,
        &metallic,
        move |value| Command::SetObjectMaterial {
            set: id,
            material: material.with_metallic(value.clamp(0.0, 1.0)),
        },
    ));
    children.push(
        grana_ui::widget::toggle_row(&theme, grana_i18n::object::object_labels(language), labels)
            .named(&format!("object.{raw}.labels"))
            .on_toggle(move |value| {
                Message::Command(Command::SetObjectLabels {
                    set: id,
                    labels: value,
                })
            }),
    );
    if labels {
        let options = label_metrics(language, state.scene, set);
        children.push(crate::inspector::form_combo(
            ctx,
            &format!("object.{raw}.label.metric"),
            grana_i18n::object::label_by(language),
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
                view::caption(ctx, &grana_i18n::object::hidden_count(language, hidden)),
            ));
            row.push(
                view::button(ctx, grana_i18n::object::show_all(language), Role::Quiet)
                    .press(Message::Command(Command::ClearHidden(id))),
            );
        }
        if let Some(object) = selected {
            row.push(
                view::button(
                    ctx,
                    grana_i18n::object::hide_selected(language),
                    Role::Quiet,
                )
                .press(Message::Command(Command::SetObjectHidden {
                    set: id,
                    time,
                    objects: vec![object],
                    hidden: true,
                })),
            );
        }
        children.push(grana_ui::row().gap(6.0).align(Align::Center).children(row));
    }
    children
}

pub(super) fn appearance<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
    id: ObjectSetId,
    width: f32,
) -> Vec<Element<'a, Message>> {
    let Some(set) = state.scene.objects.get(id) else {
        return Vec::new();
    };
    let language = ctx.language();
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
                crate::text::column(language, &measurement.label(&names)),
            )
        })
        .collect::<Vec<_>>();
    for relation in set.relations() {
        let target = state.scene.objects.get(relation.target()).map_or_else(
            || grana_i18n::object::target(language).to_string(),
            |target| target.name().to_string(),
        );
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
            let field_label = grana_i18n::measure::term(language, field.label())
                .unwrap_or_else(|| field.label().to_string());
            metrics.push((metric, format!("{target} · {field_label}{shift}")));
        }
    }
    metrics.extend(child_metrics(language, state.scene, set, &names));
    metrics.extend(coloc_metrics(language, set, &names));
    if set.classifier().is_some() {
        metrics.push((
            ObjectColor::Score,
            grana_i18n::object::classifier_score(language).to_string(),
        ));
    }
    let extent = state.scene.object_extent(set);
    let coloring = set.object_coloring().clone();
    let salt = format!("object.{}.coloring", id.raw());
    let key = Key::new(&salt);
    let open = state.workspace.disclosures.is_open(key);
    let mut children = Vec::new();
    if open {
        children.extend(crate::coloring::edit(
            ctx, &salt, id, &coloring, &metrics, extent, width,
        ));
    }
    vec![crate::inspector::disclosure(
        ctx,
        &salt,
        grana_i18n::object::coloring(language),
        open,
        children,
    )]
}
