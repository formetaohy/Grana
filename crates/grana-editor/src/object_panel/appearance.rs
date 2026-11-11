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
    let material = set.material();
    let roughness = format!("{:.2}", material.roughness());
    children.push(view::slider_readout(
        ctx,
        &format!("object.{raw}.roughness"),
        Some("Roughness"),
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
        Some("Metallic"),
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

pub(super) fn appearance<'a>(
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
    metrics.extend(child_metrics(state.scene, set, &names));
    metrics.extend(coloc_metrics(set, &names));
    if set.classifier().is_some() {
        metrics.push((ObjectColor::Score, "Classifier score".to_string()));
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
        ctx, &salt, "Coloring", open, children,
    )]
}
