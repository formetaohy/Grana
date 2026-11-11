use super::*;

pub(super) fn label_metrics(scene: &Scene, set: &ObjectSet) -> Vec<(Option<ObjectColor>, String)> {
    let channels = scene
        .dataset
        .channels()
        .iter()
        .map(|channel| channel.name().to_string())
        .collect::<Vec<_>>();
    let mut options = vec![(None, "Number".to_string())];
    options.extend(set.colorable_measurements().into_iter().map(|measurement| {
        (
            Some(ObjectColor::Measurement(measurement)),
            measurement.label(&channels),
        )
    }));
    for relation in set.relations() {
        let Some(target) = scene.objects.get(relation.target()) else {
            continue;
        };
        for field in RelationField::CONTINUOUS {
            let mut label = format!("{} {}", target.name(), field.label());
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
        child_metrics(scene, set, &channels)
            .into_iter()
            .map(|(metric, label)| (Some(metric), label)),
    );
    if set.classifier().is_some() {
        options.push((Some(ObjectColor::Score), "Classifier score".to_string()));
    }
    options
}

pub(super) fn coloc_metrics(set: &ObjectSet, channels: &[String]) -> Vec<(ObjectColor, String)> {
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
            (
                ObjectColor::Coloc { pair, metric },
                format!("{} {a}/{b}", metric.label()),
            )
        })
        .collect()
}

pub(super) fn child_metrics(
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
            options.push((
                ObjectColor::Child {
                    target: child.id(),
                    field,
                },
                format!("{} · {}", child.name(), field.label(names)),
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
    let mut elements = Vec::new();
    if open {
        for child in children {
            let Some(set) = state.scene.objects.get(child) else {
                continue;
            };
            let text = match state.scene.child_count(id, child) {
                Some(count) => format!("{} · {count} inside", set.name()),
                None => set.name().to_string(),
            };
            elements.push(view::labelled(ctx, "Child", &text));
        }
    }
    vec![crate::inspector::disclosure(
        ctx, &salt, "Children", open, elements,
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
    let mut children = vec![
        view::divider(ctx),
        view::section_header(ctx, "Selected Object", None),
    ];
    let rows = match &frame {
        ObjectFrame::Spots(spots) => {
            let Some(spot) = spots.spots().get(index).copied() else {
                return children;
            };
            let centre = spot.center();
            let intensity = frame.intensities().object(index, channel);
            vec![
                ("Object", format!("#{}", index + 1)),
                (
                    "Position",
                    format!("{:.2}, {:.2}, {:.2} µm", centre[0], centre[1], centre[2]),
                ),
                ("Diameter", format!("{:.3} µm", spot.radius() * 2.0)),
                ("Contrast", format!("{:.3}", spot.quality())),
                ("Intensity", intensity_text(intensity)),
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
                ("Object", format!("#{}", index + 1)),
                ("Volume", format!("{:.2} µm³", object.volume())),
                ("Area", format!("{:.2} µm²", object.area())),
                ("Sphericity", format!("{:.3}", object.sphericity())),
                ("Max diameter", diameter),
                ("Hull volume", hull),
                ("Ellipsoid axes", axes),
                (
                    "Centroid",
                    format!(
                        "{:.2}, {:.2}, {:.2} µm",
                        centroid[0], centroid[1], centroid[2]
                    ),
                ),
                (
                    "Extent",
                    format!("{:.2} × {:.2} × {:.2} µm", size[0], size[1], size[2]),
                ),
                ("Voxels", object.voxel_count().to_string()),
                ("Intensity", intensity_text(intensity)),
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
                ("Object", format!("#{}", index + 1)),
                ("Length", format!("{:.2} µm", object.length())),
                ("Diameter", format!("{:.2} µm", object.diameter())),
                ("Volume", format!("{:.2} µm³", object.volume())),
                ("Branches", object.branches().to_string()),
                ("Terminals", object.terminals().to_string()),
                ("Junctions", object.junctions().to_string()),
                ("Nodes", object.nodes().to_string()),
                (
                    "Centroid",
                    format!(
                        "{:.2}, {:.2}, {:.2} µm",
                        centroid[0], centroid[1], centroid[2]
                    ),
                ),
                (
                    "Extent",
                    format!("{:.2} × {:.2} × {:.2} µm", size[0], size[1], size[2]),
                ),
                ("Intensity", intensity_text(intensity)),
            ]
        }
    };
    for (label, value) in rows {
        children.push(view::labelled(ctx, label, &value));
    }
    children.push(
        view::button_with(ctx, "Delete Object", Role::Danger, Length::Fill).press(
            Message::Command(Command::DeleteObject {
                set: id,
                time,
                object: index,
            }),
        ),
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
