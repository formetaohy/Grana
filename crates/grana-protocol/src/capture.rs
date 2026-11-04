use grana_display::Coloring;
use grana_measure::{Aggregate, Group, channel_keys};
use grana_scene::{
    Measure, ObjectColor, ObjectParams, ObjectSet, Provenance, Recipe, Scene, TrackColor,
};

use crate::{
    ChannelSpec, ColocPairSpec, ColocSettingsSpec, ColocSpec, ColorSpec, FilterSpec, LassoSpec,
    MeasureSpec, PartSpec, Protocol, ProtocolError, RegionSpec, RelationSpec, RenderSpec, SetSpec,
    SnapshotSpec, SortSpec, SourceSpec, TrackSpec, TransferSpec, ViewSpec, WindowSpec,
};

impl Protocol {
    pub fn capture(
        scene: &Scene,
        measure: &Measure,
        view: Option<ViewSpec>,
    ) -> Result<Self, ProtocolError> {
        let channels = scene
            .dataset
            .channels()
            .iter()
            .map(|channel| channel.name().to_string())
            .collect::<Vec<_>>();
        let keys = channel_keys(&channels);
        let specs = scene
            .channels
            .iter()
            .zip(&channels)
            .map(|(channel, name)| ChannelSpec {
                name: name.clone(),
                enabled: channel.enabled,
                opacity: channel.opacity,
                window: WindowSpec {
                    low: channel.window.min(),
                    high: channel.window.max(),
                },
                transfer: Some(TransferSpec::of(channel.transfer())),
            })
            .collect::<Vec<_>>();
        let mask = scene
            .mask
            .and_then(|id| scene.objects.get(id))
            .map(|set| set.id());
        let mut sets = scene
            .objects
            .iter()
            .map(|set| {
                Ok(SetSpec {
                    name: set.name().to_string(),
                    color: set.color(),
                    visible: set.visible(),
                    opacity: set.opacity(),
                    color_by: object_color(set.object_coloring(), scene, &keys),
                    track_by: track_color(set.track_coloring(), scene),
                    source: source(scene, &channels, set)?,
                    mask: None,
                    track: set.track_style().map(|style| TrackSpec {
                        max_step: style.params().max_step(),
                        max_gap: style.params().max_gap(),
                        division: style.params().division(),
                        division_range: style.params().division_range(),
                        thickness: style.thickness(),
                        visible: set.track_visible(),
                    }),
                    measure: (measure.set() == Some(set.id())).then(|| MeasureSpec::of(measure)),
                })
            })
            .collect::<Result<Vec<_>, ProtocolError>>()?;
        if let Some(mask) = mask {
            let position = scene
                .objects
                .iter()
                .position(|set| set.id() == mask)
                .expect("the analysis mask names a set of the scene");
            let name = sets[position].name.clone();
            sets.rotate_left(position);
            for set in &mut sets[1..] {
                if detects(&set.source) {
                    set.mask = Some(name.clone());
                }
            }
        }
        let relations = scene
            .objects
            .iter()
            .flat_map(|set| {
                set.relations().iter().filter_map(|relation| {
                    let target = scene.objects.get(relation.target())?;
                    Some(RelationSpec {
                        source: set.name().to_string(),
                        target: target.name().to_string(),
                        shift: relation.shift(),
                    })
                })
            })
            .collect::<Vec<_>>();
        let coloc = if scene.coloc.results().is_empty() {
            None
        } else {
            let pairs = scene
                .coloc
                .results()
                .iter()
                .map(|result| {
                    let pair = result.pair();
                    ColocPairSpec {
                        a: channels[pair.a()].clone(),
                        b: channels[pair.b()].clone(),
                        thresholds: [result.thresholds().a(), result.thresholds().b()],
                    }
                })
                .collect::<Vec<_>>();
            let mask = scene
                .mask
                .and_then(|id| scene.objects.get(id))
                .map(|set| set.name().to_string());
            let settings = scene.coloc.settings();
            Some(ColocSpec {
                settings: ColocSettingsSpec {
                    permutations: settings.permutations(),
                    block: settings.block(),
                },
                pairs,
                mask,
            })
        };
        let snapshot = Some(SnapshotSpec::of(view.as_ref().and_then(ViewSpec::export)));
        Protocol {
            region: RegionSpec::of(scene.region, scene.dataset.world_size()),
            render: Some(RenderSpec::of(
                scene.shading,
                scene.exposure,
                scene.step_scale,
            )),
            channels: specs,
            sets,
            relations,
            coloc,
            view,
            snapshot,
        }
        .checked()
    }
}

fn source(
    scene: &Scene,
    channels: &[String],
    set: &ObjectSet,
) -> Result<SourceSpec, ProtocolError> {
    match set.provenance() {
        Provenance::Detection => Ok(detection(
            &channels[set.channel()],
            set.params(),
            &(0..set.frame_count())
                .map(|time| set.seeds(time).to_vec())
                .collect::<Vec<_>>(),
        )),
        Provenance::Filtered { source, criteria } => {
            let reference = scene.objects.get(*source).ok_or_else(|| {
                ProtocolError::Invalid(format!(
                    "{} filters an object set that left the scene",
                    set.name()
                ))
            })?;
            Ok(SourceSpec::Filtered {
                source: reference.name().to_string(),
                group: criteria.group().key().to_string(),
                filters: criteria
                    .filters()
                    .iter()
                    .map(|filter| FilterSpec {
                        column: filter.key().to_string(),
                        minimum: filter.minimum(),
                        maximum: filter.maximum(),
                    })
                    .collect(),
                lasso: criteria.lasso().map(|lasso| LassoSpec {
                    x: lasso.x().to_string(),
                    y: lasso.y().to_string(),
                    points: lasso.points().to_vec(),
                }),
            })
        }
        Provenance::Copied { source } => match scene.objects.get(*source) {
            Some(reference) => Ok(SourceSpec::Copy {
                source: reference.name().to_string(),
            }),
            None => Ok(detection(
                &channels[set.channel()],
                set.params(),
                &(0..set.frame_count())
                    .map(|time| set.seeds(time).to_vec())
                    .collect::<Vec<_>>(),
            )),
        },
        Provenance::Merged { parts } => Ok(SourceSpec::Merged {
            parts: parts
                .iter()
                .map(|part| part_spec(scene, channels, part))
                .collect::<Result<Vec<_>, ProtocolError>>()?,
        }),
    }
}

fn part_spec(scene: &Scene, channels: &[String], part: &Recipe) -> Result<PartSpec, ProtocolError> {
    let source = match part.provenance() {
        Provenance::Detection => detection(&channels[part.channel()], part.params(), part.seeds()),
        Provenance::Filtered { source, criteria } => {
            let reference = scene.objects.get(*source).ok_or_else(|| {
                ProtocolError::Invalid(format!(
                    "{} filters an object set that left the scene",
                    part.name()
                ))
            })?;
            SourceSpec::Filtered {
                source: reference.name().to_string(),
                group: criteria.group().key().to_string(),
                filters: criteria
                    .filters()
                    .iter()
                    .map(|filter| FilterSpec {
                        column: filter.key().to_string(),
                        minimum: filter.minimum(),
                        maximum: filter.maximum(),
                    })
                    .collect(),
                lasso: criteria.lasso().map(|lasso| LassoSpec {
                    x: lasso.x().to_string(),
                    y: lasso.y().to_string(),
                    points: lasso.points().to_vec(),
                }),
            }
        }
        Provenance::Copied { source } => match scene.objects.get(*source) {
            Some(reference) => SourceSpec::Copy {
                source: reference.name().to_string(),
            },
            None => detection(&channels[part.channel()], part.params(), part.seeds()),
        },
        Provenance::Merged { parts } => SourceSpec::Merged {
            parts: parts
                .iter()
                .map(|nested| part_spec(scene, channels, nested))
                .collect::<Result<Vec<_>, ProtocolError>>()?,
        },
    };
    Ok(PartSpec {
        name: part.name().to_string(),
        source,
    })
}

fn detection(channel: &str, params: ObjectParams, seeds: &[Vec<[f32; 3]>]) -> SourceSpec {
    match params {
        ObjectParams::Spots(params) => SourceSpec::Spots {
            channel: channel.to_string(),
            diameter: params.diameter(),
            contrast: params.threshold(),
        },
        ObjectParams::Surfaces(params) => SourceSpec::Surfaces {
            channel: channel.to_string(),
            threshold: params.threshold(),
            grain: params.grain(),
            split: params.split(),
            min_volume: params.min_volume(),
            opening: params.opening(),
            closing: params.closing(),
            min_sphericity: params.min_sphericity(),
            min_mean_intensity: params.min_mean_intensity(),
            seeds: seeds.to_vec(),
        },
        ObjectParams::Filaments(params) => SourceSpec::Filaments {
            channel: channel.to_string(),
            threshold: params.threshold(),
            min_length: params.min_length(),
        },
    }
}

fn detects(source: &SourceSpec) -> bool {
    match source {
        SourceSpec::Spots { .. } | SourceSpec::Surfaces { .. } | SourceSpec::Filaments { .. } => {
            true
        }
        SourceSpec::Filtered { .. } | SourceSpec::Copy { .. } => false,
        SourceSpec::Merged { parts } => parts.iter().any(|part| detects(&part.source)),
    }
}

fn object_color(
    coloring: &Coloring<ObjectColor>,
    scene: &Scene,
    keys: &[String],
) -> Option<ColorSpec> {
    let metric = coloring.metric()?;
    let color = ColorSpec {
        palette: coloring.palette().name().to_ascii_lowercase(),
        reversed: coloring.reversed(),
        range: coloring.manual().map(|(low, high)| [low, high]),
        key: None,
        target: None,
        field: None,
        aggregate: None,
    };
    Some(match metric {
        ObjectColor::Measurement(measurement) => ColorSpec {
            key: Some(measurement.key(keys)),
            ..color
        },
        ObjectColor::Relation { target, field } => ColorSpec {
            target: Some(scene.objects.get(target)?.name().to_string()),
            field: Some(field.key().to_string()),
            ..color
        },
    })
}

fn track_color(coloring: &Coloring<TrackColor>, scene: &Scene) -> Option<ColorSpec> {
    let metric = coloring.metric()?;
    let color = ColorSpec {
        key: None,
        target: None,
        field: None,
        aggregate: None,
        palette: coloring.palette().name().to_ascii_lowercase(),
        reversed: coloring.reversed(),
        range: coloring.manual().map(|(low, high)| [low, high]),
    };
    Some(match metric {
        TrackColor::Measurement(measurement) => ColorSpec {
            key: Some(measurement.column().key().to_string()),
            ..color
        },
        TrackColor::Relation {
            target,
            field,
            aggregate,
        } => ColorSpec {
            target: Some(scene.objects.get(target)?.name().to_string()),
            field: Some(field.key().to_string()),
            aggregate: Some(aggregate.key().to_string()),
            ..color
        },
    })
}

impl MeasureSpec {
    pub fn of(measure: &Measure) -> Self {
        let group = measure.group();
        let aggregate = if group == Group::Object {
            Aggregate::Mean
        } else {
            measure.aggregate()
        };
        let table = measure.table();
        let columns = match (table, measure.visible()) {
            (Some(table), visible) => table
                .columns()
                .iter()
                .zip(visible)
                .filter(|(_, shown)| **shown)
                .map(|(column, _)| column.key().to_string())
                .collect(),
            _ => Vec::new(),
        };
        let filters = match table {
            Some(table) => measure
                .filters()
                .iter()
                .map(|filter| {
                    let column = table
                        .columns()
                        .get(filter.column())
                        .expect("a measure filter names a column of its table");
                    FilterSpec {
                        column: column.key().to_string(),
                        minimum: filter.minimum(),
                        maximum: filter.maximum(),
                    }
                })
                .collect(),
            None => Vec::new(),
        };
        let axes = table.map(|table| {
            measure
                .axes()
                .map(|axis| table.columns()[axis].key().to_string())
        });
        let lasso = table.and_then(|table| {
            let lasso = measure.lasso()?;
            Some(LassoSpec {
                x: table.columns().get(lasso.x())?.key().to_string(),
                y: table.columns().get(lasso.y())?.key().to_string(),
                points: lasso.points().to_vec(),
            })
        });
        Self {
            group: group.key().to_string(),
            aggregate: aggregate.key().to_string(),
            scope: Some(measure.scope().label().to_string()),
            visible: measure.visibility(),
            view: Some(measure.view().name().to_string()),
            axes,
            lasso,
            filters,
            columns,
            sort: table.and_then(|table| {
                let (column, descending) = measure.sort()?;
                Some(SortSpec {
                    column: table.columns().get(column)?.key().to_string(),
                    descending,
                })
            }),
        }
    }
}
