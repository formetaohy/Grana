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
    let language = ctx.language();
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
                &grana_i18n::object::classifier_fit(
                    language,
                    classifier.features().len(),
                    classifier.positives(),
                    classifier.negatives(),
                    classifier.accuracy() * 100.0,
                ),
            )),
            None => children.push(view::weak(
                ctx,
                grana_i18n::object::classifier_intro(language),
            )),
        }
        children.push(
            view::button_with(
                ctx,
                &grana_i18n::object::measurements(language, features.len()),
                Role::SurfaceHigh,
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
                &grana_i18n::object::gate_rows(
                    language,
                    state.measure.rows().len(),
                    state.measure.total(),
                ),
            ));
        } else {
            children.push(view::weak(
                ctx,
                grana_i18n::object::classifier_gate_hint(language),
            ));
        }
        let label = if current.is_some() {
            grana_i18n::object::retrain_classifier(language)
        } else {
            grana_i18n::object::train_classifier(language)
        };
        children.push(
            view::button_with(ctx, label, Role::PrimaryContainer, Length::Fill).press(
                Message::Command(Command::TrainClassifier {
                    set: id,
                    features: features.clone(),
                }),
            ),
        );
        if current.is_some() {
            children.push(
                view::button_with(
                    ctx,
                    grana_i18n::object::clear_classifier(language),
                    Role::Error,
                    Length::Fill,
                )
                .press(Message::Command(Command::ClearClassifier(id))),
            );
        }
    }
    vec![crate::inspector::disclosure(
        ctx,
        &salt,
        grana_i18n::object::classifier(language),
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
    let content = rows.len() as f32 * 32.0 + 16.0;
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
