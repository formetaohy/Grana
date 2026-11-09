use grana_display::Coloring;
use grana_measure::{Aggregate, Group, channel_keys};
use grana_optics::{Psf, PsfParams};
use grana_scene::{
    Measure, ObjectColor, ObjectParams, ObjectSet, Provenance, Recipe, Scene, Seeds, SegmentParams,
    TrackColor,
};

use crate::{
    AnnotationSpec, ChannelSpec, ClassifierSpec, ColocPairSpec, ColocSettingsSpec, ColocSpec,
    ColorSpec, CurationSpec, DerivedSpec, DriftSpec, FilterSpec, LassoSpec, MaskFilterSpec,
    MaskSpec, MeasureSpec, MeasuredSpec, MetricSpec, PartSpec, Protocol, ProtocolError, RegionSpec,
    RelationSpec, RenderSpec, RestoreSpec, SetSpec, SnapshotSpec, SortSpec, SourceSpec,
    SurfaceEditSpec, TrackSpec, TransferSpec, ViewSpec, WindowSpec,
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
        let derived = scene
            .derived()
            .iter()
            .map(|entry| {
                let spec = entry.spec();
                let channel = scene
                    .dataset
                    .channels()
                    .get(spec.source())
                    .ok_or_else(|| {
                        ProtocolError::Invalid(format!(
                            "the derived channel {} derives from a channel of the dataset that left the scene",
                            spec.name()
                        ))
                    })?
                    .name()
                    .to_string();
                Ok(match spec {
                    grana_scene::DerivedSpec::Mask(mask) => {
                        let set = scene.objects.get(mask.set()).ok_or_else(|| {
                            ProtocolError::Invalid(format!(
                                "the mask channel {} masks an object set that left the scene",
                                mask.name()
                            ))
                        })?;
                        DerivedSpec::Mask(MaskSpec {
                            set: set.key().as_str().to_string(),
                            channel,
                            name: mask.name().to_string(),
                            tint: mask.tint(),
                        })
                    }
                    grana_scene::DerivedSpec::Restore(restore) => {
                        let intensity = restore.intensity();
                        let (params, measured) = match restore.psf() {
                            Psf::Theoretical(params) => (*params, None),
                            Psf::Measured(psf) => {
                                let mut values = Vec::new();
                                for kernel in psf.kernels() {
                                    values.extend_from_slice(kernel.values());
                                }
                                let spacing = psf.spacing();
                                (
                                    PsfParams::new(1.4, 0.52, 1.515, 1.33, 0.0),
                                    Some(MeasuredSpec {
                                        spacing: [spacing.x(), spacing.y(), spacing.z()],
                                        dims: psf.dims(),
                                        values,
                                    }),
                                )
                            }
                        };
                        DerivedSpec::Restore(RestoreSpec {
                            channel,
                            name: restore.name().to_string(),
                            tint: restore.tint(),
                            intensity: [intensity.min(), intensity.max()],
                            numerical_aperture: params.numerical_aperture(),
                            wavelength: params.wavelength(),
                            immersion: params.immersion(),
                            sample: params.sample(),
                            depth: params.depth(),
                            measured,
                            iterations: restore.iterations(),
                        })
                    }
                })
            })
            .collect::<Result<Vec<_>, ProtocolError>>()?;
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
                    key: set.key().as_str().to_string(),
                    name: set.name().to_string(),
                    color: set.color(),
                    visible: set.visible(),
                    labels: set.labels(),
                    label: set
                        .label_metric()
                        .and_then(|metric| MetricSpec::of(metric, scene, &keys)),
                    opacity: set.opacity(),
                    color_by: object_color(set.object_coloring(), scene, &keys),
                    track_by: track_color(set.track_coloring(), scene),
                    classifier: set
                        .classifier()
                        .map(|classifier| ClassifierSpec::of(classifier, &keys)),
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
                    curation: CurationSpec::of(set.curation()),
                    measure: (measure.set() == Some(set.id())).then(|| MeasureSpec::of(measure)),
                    meshes: Vec::new(),
                })
            })
            .collect::<Result<Vec<_>, ProtocolError>>()?;
        if let Some(mask) = mask {
            let key = scene
                .objects
                .get(mask)
                .expect("the analysis mask names a set of the scene")
                .key()
                .as_str()
                .to_string();
            let target = scene
                .mask_filter
                .and_then(|filter| scene.objects.get(filter.target()))
                .map(|set| set.key().as_str().to_string());
            for set in &mut sets {
                if set.key != key && target.as_deref() != Some(set.key.as_str()) {
                    set.mask = Some(key.clone());
                }
            }
        }
        let sets = ordered(sets, &derived)?;
        let relations = scene
            .objects
            .iter()
            .flat_map(|set| {
                set.relations().iter().filter_map(|relation| {
                    let target = scene.objects.get(relation.target())?;
                    Some(RelationSpec {
                        source: set.key().as_str().to_string(),
                        target: target.key().as_str().to_string(),
                        key: relation.key().to_string(),
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
                .map(|set| set.key().as_str().to_string());
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
        let drift = (!scene.drift().is_empty()).then(|| DriftSpec {
            channel: channels[scene.drift().channel()].clone(),
            reference: scene.drift().reference(),
            offsets: scene.drift().offsets().to_vec(),
        });
        let mask_filter = scene.mask_filter.and_then(|filter| {
            let source = scene.mask.and_then(|mask| scene.objects.get(mask))?;
            let target = scene.objects.get(filter.target())?;
            Some(MaskFilterSpec {
                source: source.key().as_str().to_string(),
                target: target.key().as_str().to_string(),
                shift: filter.shift(),
                field: filter.field().key().to_string(),
                minimum: filter.range()[0],
                maximum: filter.range()[1],
            })
        });
        let annotations = scene
            .annotations
            .iter()
            .map(AnnotationSpec::of)
            .collect::<Vec<_>>();
        Protocol {
            region: RegionSpec::of(scene.region, scene.dataset.world_size()),
            render: Some(RenderSpec::of(
                scene.shading,
                scene.exposure,
                scene.step_scale,
                scene.clip,
            )),
            channels: specs,
            derived,
            drift,
            mask_radius: scene.mask_radius,
            mask_filter,
            annotations,
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
        Provenance::Detection => detection(
            scene,
            &channels[set.channel()],
            set.params(),
            set.surface_seeds(),
        ),
        Provenance::Segmented { params } => {
            segments(&channels[set.channel()], params, scene, set.surface_seeds())
        }
        Provenance::Imported { label } => Ok(SourceSpec::Labels {
            label: label.clone(),
        }),
        Provenance::Interop { file } => Ok(SourceSpec::Tracks { file: file.clone() }),
        Provenance::Filtered { source, criteria } => {
            let reference = scene.objects.get(*source).ok_or_else(|| {
                ProtocolError::Invalid(format!(
                    "{} filters an object set that left the scene",
                    set.name()
                ))
            })?;
            Ok(SourceSpec::Filtered {
                source: reference.key().as_str().to_string(),
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
                source: reference.key().as_str().to_string(),
            }),
            None => detection(
                scene,
                &channels[set.channel()],
                set.params(),
                set.surface_seeds(),
            ),
        },
        Provenance::Edited { source, edits } => {
            let reference = scene.objects.get(*source).ok_or_else(|| {
                ProtocolError::Invalid(format!(
                    "{} edits an object set that left the scene",
                    set.name()
                ))
            })?;
            Ok(SourceSpec::Edited {
                source: reference.key().as_str().to_string(),
                edits: edits.iter().map(SurfaceEditSpec::of).collect(),
            })
        }
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
        Provenance::Detection => detection(
            scene,
            &channels[part.channel()],
            part.params(),
            part.seeds(),
        )?,
        Provenance::Segmented { params } => {
            segments(&channels[part.channel()], params, scene, part.seeds())?
        }
        Provenance::Imported { label } => SourceSpec::Labels {
            label: label.clone(),
        },
        Provenance::Interop { file } => SourceSpec::Tracks { file: file.clone() },
        Provenance::Filtered { source, criteria } => {
            let reference = scene.objects.get(*source).ok_or_else(|| {
                ProtocolError::Invalid(format!(
                    "{} filters an object set that left the scene",
                    part.name()
                ))
            })?;
            SourceSpec::Filtered {
                source: reference.key().as_str().to_string(),
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
                source: reference.key().as_str().to_string(),
            },
            None => detection(
                scene,
                &channels[part.channel()],
                part.params(),
                part.seeds(),
            )?,
        },
        Provenance::Edited { source, edits } => {
            let reference = scene.objects.get(*source).ok_or_else(|| {
                ProtocolError::Invalid(format!(
                    "{} edits an object set that left the scene",
                    part.name()
                ))
            })?;
            SourceSpec::Edited {
                source: reference.key().as_str().to_string(),
                edits: edits.iter().map(SurfaceEditSpec::of).collect(),
            }
        }
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
        curation: CurationSpec::of(part.curation()),
    })
}

fn detection(
    scene: &Scene,
    channel: &str,
    params: ObjectParams,
    seeds: &Seeds,
) -> Result<SourceSpec, ProtocolError> {
    match params {
        ObjectParams::Spots(params) => Ok(SourceSpec::Spots {
            channel: channel.to_string(),
            diameter: params.diameter(),
            contrast: params.threshold(),
        }),
        ObjectParams::Surfaces(params) => {
            let seed = seed_spec(scene, seeds)?;
            Ok(SourceSpec::Surfaces {
                channel: channel.to_string(),
                threshold: params.threshold(),
                grain: params.grain(),
                split: params.split(),
                min_volume: params.min_volume(),
                opening: params.opening(),
                closing: params.closing(),
                min_sphericity: params.min_sphericity(),
                min_mean_intensity: params.min_mean_intensity(),
                seeds: seed.points,
                seed_source: seed.source,
            })
        }
        ObjectParams::Filaments(params) => Ok(SourceSpec::Filaments {
            channel: channel.to_string(),
            threshold: params.threshold(),
            min_length: params.min_length(),
        }),
    }
}

fn seed_spec(scene: &Scene, seeds: &Seeds) -> Result<SeedSpec, ProtocolError> {
    match seeds {
        Seeds::Points(frames) => Ok(SeedSpec {
            points: frames.clone(),
            source: None,
        }),
        Seeds::Source(source) => {
            let source = scene.objects.get(*source).ok_or_else(|| {
                ProtocolError::Invalid(
                    "a detection seeds from an object set that left the scene".to_string(),
                )
            })?;
            Ok(SeedSpec {
                points: Vec::new(),
                source: Some(source.key().as_str().to_string()),
            })
        }
    }
}

struct SeedSpec {
    points: Vec<Vec<[f32; 3]>>,
    source: Option<String>,
}

fn segments(
    channel: &str,
    params: &SegmentParams,
    scene: &Scene,
    seeds: &Seeds,
) -> Result<SourceSpec, ProtocolError> {
    let seed = seed_spec(scene, seeds)?;
    Ok(SourceSpec::Segments {
        channel: channel.to_string(),
        cellprob: params.cellprob_threshold(),
        flow: params.flow_threshold(),
        minimum_size: params.minimum_size(),
        tile_overlap: params.tile_overlap(),
        slice_overlap: params.slice_overlap(),
        model: params.model().name().to_string(),
        seeds: seed.points,
        seed_source: seed.source,
    })
}

fn ordered(mut sets: Vec<SetSpec>, derived: &[DerivedSpec]) -> Result<Vec<SetSpec>, ProtocolError> {
    let mut ordered: Vec<SetSpec> = Vec::with_capacity(sets.len());
    while !sets.is_empty() {
        let position = sets
            .iter()
            .position(|set| {
                dependencies(set, derived)
                    .iter()
                    .all(|key| ordered.iter().any(|placed| placed.key == *key))
            })
            .ok_or_else(|| {
                ProtocolError::Invalid(
                    "the object sets of the protocol depend on each other in a cycle".to_string(),
                )
            })?;
        ordered.push(sets.remove(position));
    }
    Ok(ordered)
}

fn dependencies(set: &SetSpec, derived: &[DerivedSpec]) -> Vec<String> {
    let mut names = set.mask.iter().cloned().collect::<Vec<_>>();
    collect_dependencies(&set.source, derived, &mut names);
    names
}

fn collect_dependencies(source: &SourceSpec, derived: &[DerivedSpec], names: &mut Vec<String>) {
    if let Some(channel) = source.channel()
        && let Some(set) = mask_set(derived, channel)
    {
        names.push(set.to_string());
    }
    match source {
        SourceSpec::Surfaces { seed_source, .. } | SourceSpec::Segments { seed_source, .. } => {
            names.extend(seed_source.clone())
        }
        SourceSpec::Filtered { source, .. }
        | SourceSpec::Copy { source }
        | SourceSpec::Edited { source, .. } => {
            names.push(source.clone());
        }
        SourceSpec::Merged { parts } => {
            for part in parts {
                collect_dependencies(&part.source, derived, names);
            }
        }
        SourceSpec::Spots { .. }
        | SourceSpec::Filaments { .. }
        | SourceSpec::Labels { .. }
        | SourceSpec::Tracks { .. } => {}
    }
}

fn mask_set<'a>(derived: &'a [DerivedSpec], channel: &str) -> Option<&'a str> {
    derived.iter().find_map(|spec| match spec {
        DerivedSpec::Mask(mask) if mask.name == channel => Some(mask.set.as_str()),
        DerivedSpec::Mask(_) | DerivedSpec::Restore(_) => None,
    })
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
        child: false,
        shift: 0,
    };
    Some(match metric {
        ObjectColor::Score => ColorSpec {
            key: Some(grana_measure::SCORE_KEY.to_string()),
            ..color
        },
        ObjectColor::Measurement(measurement) => ColorSpec {
            key: Some(measurement.key(keys)),
            ..color
        },
        ObjectColor::Relation {
            target,
            field,
            shift,
        } => ColorSpec {
            target: Some(scene.objects.get(target)?.key().as_str().to_string()),
            field: Some(field.key().to_string()),
            shift,
            ..color
        },
        ObjectColor::Child { target, field } => ColorSpec {
            key: Some(
                field
                    .metric()
                    .map_or_else(|| "count".to_string(), |metric| metric.key(keys)),
            ),
            target: Some(scene.objects.get(target)?.key().as_str().to_string()),
            aggregate: field
                .statistic()
                .map(|statistic| statistic.key().to_string()),
            child: true,
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
        child: false,
        shift: 0,
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
            shift,
        } => ColorSpec {
            target: Some(scene.objects.get(target)?.key().as_str().to_string()),
            field: Some(field.key().to_string()),
            aggregate: Some(aggregate.key().to_string()),
            shift,
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
