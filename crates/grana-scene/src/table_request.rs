use std::collections::HashMap;
use std::sync::Arc;

use grana_measure::{
    Aggregate, Child, ChildField, ColocMetric, Frame, FrameSource, Group, Kind, ObjectColoc,
    Objects, RelationRow, Table, channel_keys,
};
use grana_object::{RelationFrame, TrackSet};

use crate::object::{ObjectFrame, ObjectKind};
use crate::object_set::ObjectSetId;
use crate::objects::members_of;
use crate::scene::Scene;
use crate::source::ObjectSource;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MeasureKey {
    set: ObjectSetId,
    revision: u64,
    group: Group,
    aggregate: Aggregate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BasisKey {
    set: ObjectSetId,
    revision: u64,
}

impl MeasureKey {
    pub fn of(scene: &Scene, set: ObjectSetId, group: Group, aggregate: Aggregate) -> Option<Self> {
        Some(Self {
            set,
            revision: scene.table_revision(set)?,
            group,
            aggregate,
        })
    }

    pub fn basis(self) -> BasisKey {
        BasisKey {
            set: self.set,
            revision: self.revision,
        }
    }

    pub fn group(self) -> Group {
        self.group
    }

    pub fn aggregate(self) -> Aggregate {
        self.aggregate
    }
}

struct RelationSource {
    key: String,
    target: String,
    shift: i32,
    frames: Vec<Option<Arc<RelationFrame>>>,
}

struct ChildSource {
    key: String,
    name: String,
    fields: Vec<ChildField>,
    objects: ObjectSource,
    members: Vec<Option<Arc<RelationFrame>>>,
}

struct TableSource {
    kind: Kind,
    channels: Vec<String>,
    relations: Vec<RelationSource>,
    children: Vec<ChildSource>,
    objects: ObjectSource,
}

impl FrameSource for TableSource {
    fn frames(&self) -> u32 {
        self.objects.frames()
    }

    fn kind(&self) -> Kind {
        self.kind
    }

    fn channels(&self) -> &[String] {
        &self.channels
    }

    fn relations(&self) -> Vec<RelationRow<'_>> {
        self.relations
            .iter()
            .map(|relation| RelationRow::new(&relation.key, &relation.target, relation.shift, None))
            .collect()
    }

    fn children(&self) -> Vec<Child<'_>> {
        self.children
            .iter()
            .map(|child| Child::new(&child.key, &child.name, child.fields.clone(), None, None))
            .collect()
    }

    fn visit(&self, time: u32, visit: &mut dyn FnMut(Frame<'_>)) {
        let Some(objects) = self.objects.frame(time) else {
            return;
        };
        let relation_frames = self
            .relations
            .iter()
            .map(|relation| relation.frames.get(time as usize).cloned().flatten())
            .collect::<Vec<Option<Arc<RelationFrame>>>>();
        let relations = self
            .relations
            .iter()
            .zip(&relation_frames)
            .map(|(relation, frame)| {
                RelationRow::new(
                    &relation.key,
                    &relation.target,
                    relation.shift,
                    frame.as_deref(),
                )
            })
            .collect::<Vec<_>>();
        let child_frames = self
            .children
            .iter()
            .map(|child| child.objects.frame(time))
            .collect::<Vec<Option<Arc<ObjectFrame>>>>();
        let children = self
            .children
            .iter()
            .zip(&child_frames)
            .map(|(child, frame)| {
                let members = frame.as_ref().and_then(|frame| {
                    child
                        .members
                        .get(time as usize)
                        .and_then(Option::as_deref)
                        .filter(|relation| relation.objects() == frame.object_count())
                        .map(members_of)
                });
                Child::new(
                    &child.key,
                    &child.name,
                    child.fields.clone(),
                    frame.as_ref().map(|frame| frame.objects()),
                    members,
                )
            })
            .collect::<Vec<_>>();
        visit(Frame::new(time, objects.objects(), &relations, &children));
    }
}

pub struct TableRequest {
    key: MeasureKey,
    source: TableSource,
    tracks: Option<TrackSet>,
    frame_interval: f32,
    classifier: Option<grana_measure::Classifier>,
    coloc: Option<ObjectColoc>,
    basis: Option<Arc<Table>>,
    retained: Vec<String>,
}

impl TableRequest {
    pub fn new(scene: &Scene, id: ObjectSetId, group: Group, aggregate: Aggregate) -> Option<Self> {
        let set = scene.objects.get(id)?;
        if group == Group::Molecule && !crate::measure::molecule_grouping(scene, id) {
            return None;
        }
        if group == Group::Track && set.tracks().is_none() {
            return None;
        }
        if set.detected() == 0 {
            return None;
        }
        let relations = set
            .relations()
            .iter()
            .map(|relation| RelationSource {
                key: relation.key().to_string(),
                target: scene
                    .objects
                    .get(relation.target())
                    .map_or_else(|| "target".to_string(), |target| target.name().to_string()),
                shift: relation.shift(),
                frames: relation.shared_frames().to_vec(),
            })
            .collect();
        let children = match group {
            Group::Object => scene
                .objects
                .child_sets(id)
                .into_iter()
                .filter_map(|child| child_source(&scene.objects, id, child))
                .collect(),
            Group::Track | Group::Timepoint | Group::Molecule => Vec::new(),
        };
        let channels = scene.dataset.channels()[..set.measured_channels()]
            .iter()
            .map(|channel| channel.name().to_string())
            .collect::<Vec<_>>();
        Some(Self {
            key: MeasureKey::of(scene, id, group, aggregate)?,
            source: TableSource {
                kind: kind_of(set.kind()),
                channels,
                relations,
                children,
                objects: set.plain_source(),
            },
            tracks: set.tracks().map(|layer| layer.set().clone()),
            frame_interval: scene.dataset.frame_interval(),
            classifier: set.classifier().cloned(),
            coloc: set.object_coloc().cloned(),
            basis: None,
            retained: Vec::new(),
        })
    }

    pub fn with_basis(mut self, basis: Arc<Table>) -> Self {
        self.basis = Some(basis);
        self
    }

    pub fn with_retained(mut self, retained: Vec<String>) -> Self {
        self.retained = retained;
        self
    }

    pub fn key(&self) -> MeasureKey {
        self.key
    }

    pub fn build(self) -> Option<Table> {
        let Self {
            key,
            source,
            tracks,
            frame_interval,
            classifier,
            coloc,
            basis,
            retained,
        } = self;
        let keys = channel_keys(source.channels());
        let source: Arc<dyn FrameSource> = Arc::new(source);
        let tracks = tracks.map(Arc::new);
        let group = key.group();
        let aggregate = key.aggregate();
        let mut table = Table::build(
            Arc::clone(&source),
            tracks.clone(),
            frame_interval,
            group,
            aggregate,
            basis,
        )?;
        if let Some(classifier) = &classifier {
            let column = match group {
                Group::Object => Some((classifier.column(), classifier.scores(&table, &keys))),
                Group::Track | Group::Timepoint | Group::Molecule => Some((
                    classifier.aggregate_column(aggregate),
                    aggregate_scores(
                        source.as_ref(),
                        tracks.as_deref(),
                        aggregate,
                        group,
                        &table,
                        classifier,
                    ),
                )),
            };
            if let Some((column, values)) = column {
                table = table.with_column(column, values);
            }
        }
        if let Some(coloc) = &coloc {
            for metric in ColocMetric::ALL {
                let column = metric.column(coloc.pair(), source.channels(), &keys);
                let values = match group {
                    Group::Object => (0..table.rows())
                        .map(|row| {
                            let observation = table.observation(row);
                            let (Some(time), Some(object)) =
                                (observation.time(), observation.object())
                            else {
                                return f32::NAN;
                            };
                            coloc
                                .metric(time, object, metric)
                                .map_or(f32::NAN, |value| value as f32)
                        })
                        .collect(),
                    Group::Track | Group::Timepoint | Group::Molecule => aggregate_coloc(
                        source.as_ref(),
                        tracks.as_deref(),
                        aggregate,
                        group,
                        &table,
                        coloc,
                        metric,
                    ),
                };
                table = table.with_column(column, values);
            }
        }
        let resident = if retained.is_empty() {
            crate::measure::default_retained(&table)
        } else {
            retained
                .iter()
                .filter_map(|key| table.column(key))
                .collect()
        };
        table.retain(resident.iter().copied());
        table.materialize_many(&resident);
        Some(table)
    }
}

fn aggregate_scores(
    source: &dyn FrameSource,
    tracks: Option<&TrackSet>,
    aggregate: Aggregate,
    group: Group,
    table: &Table,
    classifier: &grana_measure::Classifier,
) -> Vec<f32> {
    match group {
        Group::Object => Vec::new(),
        Group::Track => {
            let Some(tracks) = tracks else {
                return Vec::new();
            };
            let mut collected = vec![Vec::<f32>::new(); tracks.len()];
            source.visit_all(&mut |frame: Frame<'_>| {
                for object in 0..frame.objects().count() {
                    let Some(track) = tracks.track_at(frame.time(), object) else {
                        continue;
                    };
                    collected[track].push(classifier.score(frame.objects(), object));
                }
            });
            collected
                .iter()
                .map(|values| aggregate.apply(values))
                .collect()
        }
        Group::Timepoint => {
            let rows = (0..table.rows())
                .filter_map(|row| table.observation(row).time().map(|time| (time, row)))
                .collect::<HashMap<_, _>>();
            let mut collected = vec![Vec::<f32>::new(); table.rows()];
            source.visit_all(&mut |frame: Frame<'_>| {
                let Some(row) = rows.get(&frame.time()).copied() else {
                    return;
                };
                let values = &mut collected[row];
                for object in 0..frame.objects().count() {
                    values.push(classifier.score(frame.objects(), object));
                }
            });
            collected
                .iter()
                .map(|values| aggregate.apply(values))
                .collect()
        }
        Group::Molecule => aggregate_members(source, aggregate, table, |_, objects, index| {
            Some(classifier.score(objects, index))
        }),
    }
}

fn aggregate_coloc(
    source: &dyn FrameSource,
    tracks: Option<&TrackSet>,
    aggregate: Aggregate,
    group: Group,
    table: &Table,
    coloc: &ObjectColoc,
    metric: ColocMetric,
) -> Vec<f32> {
    match group {
        Group::Object => Vec::new(),
        Group::Track => {
            let Some(tracks) = tracks else {
                return Vec::new();
            };
            (0..table.rows())
                .map(|row| {
                    let Some(track) = table.observation(row).track() else {
                        return f32::NAN;
                    };
                    let Some(track) = tracks.track(track) else {
                        return f32::NAN;
                    };
                    let values = track
                        .nodes()
                        .iter()
                        .filter_map(|node| {
                            coloc
                                .metric(node.time(), node.object(), metric)
                                .map(|v| v as f32)
                        })
                        .collect::<Vec<_>>();
                    aggregate.apply(&values)
                })
                .collect()
        }
        Group::Timepoint => (0..table.rows())
            .map(|row| {
                let Some(time) = table.observation(row).time() else {
                    return f32::NAN;
                };
                let values = coloc.objects(time).unwrap_or_default();
                let collected = values
                    .iter()
                    .filter_map(|value| value.metric(metric).map(|value| value as f32))
                    .collect::<Vec<_>>();
                aggregate.apply(&collected)
            })
            .collect(),
        Group::Molecule => aggregate_members(source, aggregate, table, |time, _, index| {
            coloc.metric(time, index, metric).map(|value| value as f32)
        }),
    }
}

fn aggregate_members(
    source: &dyn FrameSource,
    aggregate: Aggregate,
    table: &Table,
    value: impl Fn(u32, Objects<'_>, usize) -> Option<f32>,
) -> Vec<f32> {
    let mut rows = HashMap::with_capacity(table.rows());
    for row in 0..table.rows() {
        if let Some(molecule) = table.observation(row).molecule() {
            rows.insert(molecule, row);
        }
    }
    let mut collected = vec![Vec::<f32>::new(); table.rows()];
    source.visit_all(&mut |frame: Frame<'_>| {
        let objects = frame.objects();
        for index in 0..objects.count() {
            let Some(molecule) = objects.molecule(index) else {
                continue;
            };
            let Some(at) = rows.get(&molecule).copied() else {
                continue;
            };
            if let Some(score) = value(frame.time(), objects, index) {
                collected[at].push(score);
            }
        }
    });
    collected
        .iter()
        .map(|values| aggregate.apply(values))
        .collect()
}

fn kind_of(kind: ObjectKind) -> Kind {
    match kind {
        ObjectKind::Spots => Kind::Spots,
        ObjectKind::Surfaces => Kind::Surfaces,
        ObjectKind::Filaments => Kind::Filaments,
        ObjectKind::Cells => Kind::Cells,
    }
}

fn child_source(
    objects: &crate::objects::Objects,
    parent: ObjectSetId,
    child: ObjectSetId,
) -> Option<ChildSource> {
    let set = objects.get(child)?;
    let relation = set.relation(parent, 0)?;
    Some(ChildSource {
        key: set.key().as_str().to_string(),
        name: set.name().to_string(),
        fields: set.child_fields(),
        objects: set.plain_source(),
        members: relation.shared_frames().to_vec(),
    })
}
