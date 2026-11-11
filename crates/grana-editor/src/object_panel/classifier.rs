use super::*;

pub(super) fn classifier<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
    id: ObjectSetId,
    width: f32,
) -> Vec<Element<'a, Message>> {
    let Some(set) = state.scene.objects.get(id) else {
        return Vec::new();
    };
    let available = set.colorable_measurements();
    if available.is_empty() {
        return Vec::new();
    }
    let salt = format!("object.{}.classifier", id.raw());
    let features_key = format!("{salt}.features");
    let key = Key::new(&salt);
    let open = state.workspace.disclosures.is_open(key);
    let features = state.workspace.classifier_features(set);
    let current = set.classifier();
    let gated = state.measure.set() == Some(id)
        && state.measure.group() == Group::Object
        && (!state.measure.filters().is_empty() || state.measure.lasso().is_some());
    let mut children = Vec::new();
    if open {
        match current {
            Some(classifier) => children.push(view::caption(
                ctx,
                &format!(
                    "{} measurements · {} kept / {} excluded · {:.0}% balanced fit",
                    classifier.features().len(),
                    classifier.positives(),
                    classifier.negatives(),
                    classifier.accuracy() * 100.0
                ),
            )),
            None => children.push(view::weak(
                ctx,
                "Trains on the objects kept by the statistics gate and scores every object of the set.",
            )),
        }
        children.push(
            view::button_with(
                ctx,
                &format!("{} Measurements", features.len()),
                Role::Control,
                Length::Fill,
            )
            .named(&features_key)
            .press(Message::Noop),
        );
        children.push(classifier_menu(
            ctx, state, id, &available, &features, width,
        ));
        if gated {
            children.push(view::weak(
                ctx,
                &format!(
                    "The gate keeps {} of {} rows.",
                    state.measure.rows().len(),
                    state.measure.total()
                ),
            ));
        } else {
            children.push(view::weak(
                ctx,
                "Open Statistics for this set and add a filter or lasso to choose the kept objects.",
            ));
        }
        let label = if current.is_some() {
            "Retrain Classifier"
        } else {
            "Train Classifier"
        };
        children.push(
            view::button_with(ctx, label, Role::Tinted, Length::Fill).press(Message::Command(
                Command::TrainClassifier {
                    set: id,
                    features: features.clone(),
                },
            )),
        );
        if current.is_some() {
            children.push(
                view::button_with(ctx, "Clear Classifier", Role::Danger, Length::Fill)
                    .press(Message::Command(Command::ClearClassifier(id))),
            );
        }
    }
    vec![crate::inspector::disclosure(
        ctx,
        &salt,
        "Classifier",
        open,
        children,
    )]
}

pub(super) fn classifier_menu<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
    id: ObjectSetId,
    available: &[Measurement],
    selected: &[Measurement],
    width: f32,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let names = state
        .scene
        .dataset
        .channels()
        .iter()
        .map(|channel| channel.name().to_string())
        .collect::<Vec<_>>();
    let key = format!("object.{}.classifier.features", id.raw());
    let mut rows = Vec::with_capacity(available.len());
    for (index, measurement) in available.iter().enumerate() {
        let chosen = selected.contains(measurement);
        let next = available
            .iter()
            .filter(|candidate| {
                if *candidate == measurement {
                    !chosen
                } else {
                    selected.contains(*candidate)
                }
            })
            .cloned()
            .collect::<Vec<_>>();
        rows.push(
            grana_ui::widget::menu_row(&theme, &measurement.label(&names), chosen)
                .key(Key::index(&key, index))
                .press(Message::SetClassifierFeatures {
                    set: id,
                    features: next,
                }),
        );
    }
    let content = rows.len() as f32 * 28.0 + 16.0;
    let list = Element::new(grana_ui::Layout::column())
        .named(&format!("{key}.list"))
        .scroll(
            grana_ui::Axis::Column,
            Length::Fixed(content.min(240.0)),
            Some(content),
        )
        .clipped()
        .children(rows);
    grana_ui::popover::popup(Key::new(&key), width.max(200.0), [list])
}
