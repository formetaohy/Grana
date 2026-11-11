use super::*;

pub(super) fn label_metrics(
    language: Language,
    scene: &Scene,
    set: &ObjectSet,
) -> Vec<(Option<ObjectColor>, String)> {
    let channels = scene
        .dataset
        .channels()
        .iter()
        .map(|channel| channel.name().to_string())
        .collect::<Vec<_>>();
    let mut options = vec![(None, grana_i18n::object::number(language).to_string())];
    options.extend(set.colorable_measurements().into_iter().map(|measurement| {
        (
            Some(ObjectColor::Measurement(measurement)),
            crate::text::column(language, &measurement.label(&channels)),
        )
    }));
    for relation in set.relations() {
        let Some(target) = scene.objects.get(relation.target()) else {
            continue;
        };
        for field in RelationField::CONTINUOUS {
            let field_label = grana_i18n::measure::term(language, field.label())
                .unwrap_or_else(|| field.label().to_string());
            let mut label = format!("{} {}", target.name(), field_label);
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
    options.extend(
        child_metrics(language, scene, set, &channels)
            .into_iter()
            .map(|(metric, label)| (Some(metric), label)),
    );
    if set.classifier().is_some() {
        options.push((
            Some(ObjectColor::Score),
            grana_i18n::object::classifier_score(language).to_string(),
        ));
    }
    options
}

pub(super) fn coloc_metrics(
    language: Language,
    set: &ObjectSet,
    channels: &[String],
) -> Vec<(ObjectColor, String)> {
    let Some(coloc) = set.object_coloc() else {
        return Vec::new();
    };
    let pair = coloc.pair();
    let name = |channel: usize| {
        channels
            .get(channel)
            .map_or_else(|| channel.to_string(), Clone::clone)
    };
    let (a, b) = (name(pair.a()), name(pair.b()));
    ColocMetric::ALL
        .into_iter()
        .map(|metric| {
            let label = match metric {
                ColocMetric::Pearson => metric.label(),
                ColocMetric::Overlap => grana_i18n::object::overlap(language),
            };
            (
                ObjectColor::Coloc { pair, metric },
                format!("{label} {a}/{b}"),
            )
        })
        .collect()
}

pub(super) fn child_metrics(
    language: Language,
    scene: &Scene,
    set: &ObjectSet,
    names: &[String],
) -> Vec<(ObjectColor, String)> {
    let mut options = Vec::new();
    for child in scene.child_sets(set.id()) {
        let Some(child) = scene.objects.get(child) else {
            continue;
        };
        for field in child.child_fields() {
            let label = match field {
                ChildField::Count => grana_i18n::object::count(language).to_string(),
                ChildField::Stat { .. } => crate::text::column(language, &field.label(names)),
            };
            options.push((
                ObjectColor::Child {
                    target: child.id(),
                    field,
                },
                format!("{} · {label}", child.name()),
            ));
        }
    }
    options
}

pub(super) fn children_section<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
    id: ObjectSetId,
) -> Vec<Element<'a, Message>> {
    let children = state.scene.child_sets(id);
    if children.is_empty() {
        return Vec::new();
    }
    let salt = format!("object.{}.children", id.raw());
    let key = Key::new(&salt);
    let open = state.workspace.disclosures.is_open(key);
    let language = ctx.language();
    let mut elements = Vec::new();
    if open {
        for child in children {
            let Some(set) = state.scene.objects.get(child) else {
                continue;
            };
            let text = match state.scene.child_count(id, child) {
                Some(count) => grana_i18n::object::child_inside(language, set.name(), count),
                None => set.name().to_string(),
            };
            elements.push(view::labelled(
                ctx,
                grana_i18n::object::child(language),
                &text,
            ));
        }
    }
    vec![crate::inspector::disclosure(
        ctx,
        &salt,
        grana_i18n::object::children(language),
        open,
        elements,
    )]
}

pub(super) fn measure<'a>(
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
    let language = ctx.language();
    let property = |english: &str| grana_i18n::object::property(language, english);
    let mut children = vec![
        view::divider(ctx),
        view::section_header(ctx, grana_i18n::object::selected_object(language), None),
    ];
    let rows = match &frame {
        ObjectFrame::Spots(spots) => {
            let Some(spot) = spots.spots().get(index).copied() else {
                return children;
            };
            let centre = spot.center();
            let intensity = frame.intensities().object(index, channel);
            vec![
                (property("Object"), format!("#{}", index + 1)),
                (
                    property("Position"),
                    format!("{:.2}, {:.2}, {:.2} µm", centre[0], centre[1], centre[2]),
                ),
                (
                    property("Diameter"),
                    format!("{:.3} µm", spot.radius() * 2.0),
                ),
                (property("Contrast"), format!("{:.3}", spot.quality())),
                (property("Intensity"), intensity_text(intensity)),
            ]
        }
        ObjectFrame::Surfaces(surfaces) => {
            let Some(object) = surfaces.object(index) else {
                return children;
            };
            let intensity = surfaces.intensities().object(index, channel);
            let centroid = object.centroid();
            let size = object.bounds().size();
            let (diameter, hull, axes) = match object.shape() {
                Some(shape) => (
                    format!("{:.3} µm", shape.diameter()),
                    format!("{:.2} µm³", shape.hull_volume()),
                    {
                        let axes = shape.axes();
                        format!("{:.2} × {:.2} × {:.2} µm", axes[0], axes[1], axes[2])
                    },
                ),
                None => ("—".to_string(), "—".to_string(), "—".to_string()),
            };
            vec![
                (property("Object"), format!("#{}", index + 1)),
                (property("Volume"), format!("{:.2} µm³", object.volume())),
                (property("Area"), format!("{:.2} µm²", object.area())),
                (
                    property("Sphericity"),
                    format!("{:.3}", object.sphericity()),
                ),
                (property("Max diameter"), diameter),
                (property("Hull volume"), hull),
                (property("Ellipsoid axes"), axes),
                (
                    property("Centroid"),
                    format!(
                        "{:.2}, {:.2}, {:.2} µm",
                        centroid[0], centroid[1], centroid[2]
                    ),
                ),
                (
                    property("Extent"),
                    format!("{:.2} × {:.2} × {:.2} µm", size[0], size[1], size[2]),
                ),
                (property("Voxels"), object.voxel_count().to_string()),
                (property("Intensity"), intensity_text(intensity)),
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
                (property("Object"), format!("#{}", index + 1)),
                (property("Length"), format!("{:.2} µm", object.length())),
                (property("Diameter"), format!("{:.2} µm", object.diameter())),
                (property("Volume"), format!("{:.2} µm³", object.volume())),
                (property("Branches"), object.branches().to_string()),
                (property("Terminals"), object.terminals().to_string()),
                (property("Junctions"), object.junctions().to_string()),
                (property("Nodes"), object.nodes().to_string()),
                (
                    property("Centroid"),
                    format!(
                        "{:.2}, {:.2}, {:.2} µm",
                        centroid[0], centroid[1], centroid[2]
                    ),
                ),
                (
                    property("Extent"),
                    format!("{:.2} × {:.2} × {:.2} µm", size[0], size[1], size[2]),
                ),
                (property("Intensity"), intensity_text(intensity)),
            ]
        }
    };
    for (label, value) in rows {
        children.push(view::labelled(ctx, &label, &value));
    }
    children.push(
        view::button_with(
            ctx,
            grana_i18n::object::delete_object(language),
            Role::Danger,
            Length::Fill,
        )
        .press(Message::Command(Command::DeleteObject {
            set: id,
            time,
            object: index,
        })),
    );
    children
}

pub(super) fn intensity_text(intensity: grana_object::SampleStatistics) -> String {
    format!(
        "{:.0} ± {:.0} ({:.0}–{:.0})",
        intensity.mean(),
        intensity.deviation(),
        intensity.minimum(),
        intensity.maximum(),
    )
}
