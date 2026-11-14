use std::collections::HashMap;
use std::sync::Arc;

use grana_measure::{Aggregate, Child, ChildField, ColocMetric, Frame, Group, ObjectColoc};
use grana_measure::{RelationRow, Table};
use grana_object::{RelationFrame, TrackSet};

use crate::object::ObjectFrame;
use crate::object_set::ObjectSetId;
use crate::objects::{Objects, members_of};
use crate::scene::Scene;

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
}

#[derive(Clone)]
struct Relation {
    key: String,
    target: String,
    shift: i32,
    frame: Option<Arc<RelationFrame>>,
}

#[derive(Clone)]
struct ChildSet {
    key: String,
    name: String,
    fields: Vec<ChildField>,
    frame: Option<Arc<ObjectFrame>>,
    members: Option<Vec<u32>>,
}

struct Row {
    time: u32,
    objects: Arc<ObjectFrame>,
    relations: Vec<Relation>,
    children: Vec<ChildSet>,
}

pub struct TableRequest {
    key: MeasureKey,
    rows: Vec<Row>,
    tracks: Option<TrackSet>,
    channels: Vec<String>,
    frame_interval: f32,
    classifier: Option<grana_measure::Classifier>,
    coloc: Option<ObjectColoc>,
    basis: Option<Arc<Table>>,
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
        let children = match group {
            Group::Object => scene.objects.child_sets(id),
            Group::Track | Group::Timepoint | Group::Molecule => Vec::new(),
        };
        let mut rows = Vec::new();
        for time in 0..set.frame_count() {
            let Some(objects) = set.frame_shared(time) else {
                continue;
            };
            let relations = set
                .relations()
                .iter()
                .map(|relation| Relation {
                    key: relation.key().to_string(),
                    target: scene
                        .objects
                        .get(relation.target())
                        .map_or_else(|| "target".to_string(), |target| target.name().to_string()),
                    shift: relation.shift(),
                    frame: relation.frame_shared(time),
                })
                .collect();
            let children = children
                .iter()
                .filter_map(|child| child_row(&scene.objects, id, *child, time))
                .collect();
            rows.push(Row {
                time,
                objects,
                relations,
                children,
            });
        }
        let channels = scene.dataset.channels()[..set.measured_channels()]
            .iter()
            .map(|channel| channel.name().to_string())
            .collect::<Vec<_>>();
        Some(Self {
            key: MeasureKey::of(scene, id, group, aggregate)?,
            rows,
            tracks: set.tracks().map(|layer| layer.set().clone()),
            channels,
            frame_interval: scene.dataset.frame_interval(),
            classifier: set.classifier().cloned(),
            coloc: set.object_coloc().cloned(),
            basis: None,
        })
    }

    pub fn with_basis(mut self, basis: Arc<Table>) -> Self {
        self.basis = Some(basis);
        self
    }

    pub fn key(&self) -> MeasureKey {
        self.key
    }

    pub fn build(&self) -> Option<Table> {
        if self.rows.is_empty() {
            return None;
        }
        let mut rows = Vec::with_capacity(self.rows.len());
        for row in &self.rows {
            let relations = row
                .relations
                .iter()
                .map(|relation| {
                    RelationRow::new(
                        &relation.key,
                        &relation.target,
                        relation.shift,
                        relation.frame.as_deref(),
                    )
                })
                .collect::<Vec<_>>();
            let children = row
                .children
                .iter()
                .map(|child| {
                    Child::new(
                        &child.key,
                        &child.name,
                        child.fields.clone(),
                        child.frame.as_deref().map(ObjectFrame::objects),
                        child.members.clone(),
                    )
                })
                .collect::<Vec<_>>();
            rows.push((relations, children));
        }
        let frames = self
            .rows
            .iter()
            .zip(&rows)
            .map(|(row, (relations, children))| {
                Frame::new(row.time, row.objects.objects(), relations, children)
            })
            .collect::<Vec<_>>();
        let mut table = Table::build(
            &frames,
            self.tracks.as_ref(),
            &self.channels,
            self.frame_interval,
            self.key.group,
            self.key.aggregate,
            self.basis.clone(),
        );
        let keys = grana_measure::channel_keys(&self.channels);
        if let Some(classifier) = &self.classifier {
            let column = match self.key.group {
                Group::Object => Some((classifier.column(), classifier.scores(&table, &keys))),
                Group::Track | Group::Timepoint | Group::Molecule => Some((
                    classifier.aggregate_column(self.key.aggregate),
                    self.aggregate_scores(&table, classifier),
                )),
            };
            if let Some((column, values)) = column {
                table = table.with_column(column, values);
            }
        }
        if let Some(coloc) = &self.coloc {
            for metric in ColocMetric::ALL {
                let column = metric.column(coloc.pair(), &self.channels, &keys);
                let values = match self.key.group {
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
                    Group::Track | Group::Timepoint | Group::Molecule => {
                        self.aggregate_coloc(&table, coloc, metric)
                    }
                };
                table = table.with_column(column, values);
            }
        }
        Some(table)
    }

    fn aggregate_scores(&self, table: &Table, classifier: &grana_measure::Classifier) -> Vec<f32> {
        let aggregate = self.key.aggregate;
        match self.key.group {
            Group::Object => Vec::new(),
            Group::Track => {
                let Some(tracks) = self.tracks.as_ref() else {
                    return Vec::new();
                };
                let mut collected = vec![Vec::<f32>::new(); tracks.len()];
                for row in &self.rows {
                    let objects = row.objects.objects();
                    for index in 0..row.objects.object_count() {
                        let Some(track) = tracks.track_at(row.time, index) else {
                            continue;
                        };
                        collected[track].push(classifier.score(objects, index));
                    }
                }
                collected
                    .iter()
                    .map(|values| aggregate.apply(values))
                    .collect()
            }
            Group::Timepoint => self
                .rows
                .iter()
                .map(|row| {
                    let objects = row.objects.objects();
                    let mut collected = Vec::with_capacity(row.objects.object_count());
                    for index in 0..row.objects.object_count() {
                        collected.push(classifier.score(objects, index));
                    }
                    aggregate.apply(&collected)
                })
                .collect(),
            Group::Molecule => self.aggregate_members(table, |row, index| {
                Some(classifier.score(row.objects.objects(), index))
            }),
        }
    }

    fn aggregate_coloc(&self, table: &Table, coloc: &ObjectColoc, metric: ColocMetric) -> Vec<f32> {
        let aggregate = self.key.aggregate;
        match self.key.group {
            Group::Object => Vec::new(),
            Group::Track => {
                let Some(tracks) = self.tracks.as_ref() else {
                    return Vec::new();
                };
                let mut collected = vec![Vec::<f32>::new(); tracks.len()];
                for row in &self.rows {
                    let Some(values) = coloc.objects(row.time) else {
                        continue;
                    };
                    for (index, value) in values.iter().enumerate().take(row.objects.object_count())
                    {
                        let Some(value) = value.metric(metric) else {
                            continue;
                        };
                        let Some(track) = tracks.track_at(row.time, index) else {
                            continue;
                        };
                        collected[track].push(value as f32);
                    }
                }
                collected
                    .iter()
                    .map(|values| aggregate.apply(values))
                    .collect()
            }
            Group::Timepoint => self
                .rows
                .iter()
                .map(|row| {
                    let mut collected = Vec::with_capacity(row.objects.object_count());
                    let values = coloc.objects(row.time).unwrap_or_default();
                    for value in values.iter().take(row.objects.object_count()) {
                        if let Some(value) = value.metric(metric) {
                            collected.push(value as f32);
                        }
                    }
                    aggregate.apply(&collected)
                })
                .collect(),
            Group::Molecule => self.aggregate_members(table, |row, index| {
                coloc
                    .objects(row.time)?
                    .get(index)?
                    .metric(metric)
                    .map(|value| value as f32)
            }),
        }
    }

    fn aggregate_members(
        &self,
        table: &Table,
        value: impl Fn(&Row, usize) -> Option<f32>,
    ) -> Vec<f32> {
        let mut rows = HashMap::with_capacity(table.rows());
        for row in 0..table.rows() {
            if let Some(molecule) = table.observation(row).molecule() {
                rows.insert(molecule, row);
            }
        }
        let mut collected = vec![Vec::<f32>::new(); table.rows()];
        for row in &self.rows {
            let objects = row.objects.objects();
            for index in 0..row.objects.object_count() {
                let Some(molecule) = objects.molecule(index) else {
                    continue;
                };
                let Some(at) = rows.get(&molecule).copied() else {
                    continue;
                };
                if let Some(value) = value(row, index) {
                    collected[at].push(value);
                }
            }
        }
        let aggregate = self.key.aggregate;
        collected
            .iter()
            .map(|values| aggregate.apply(values))
            .collect()
    }
}

fn child_row(
    objects: &Objects,
    parent: ObjectSetId,
    child: ObjectSetId,
    time: u32,
) -> Option<ChildSet> {
    let set = objects.get(child)?;
    let relation = set.relation(parent, 0)?;
    let frame = set.frame_shared(time);
    let members = frame.as_ref().and_then(|frame| {
        relation
            .frame(time)
            .filter(|relation| relation.objects() == frame.object_count())
            .map(members_of)
    });
    Some(ChildSet {
        key: set.key().as_str().to_string(),
        name: set.name().to_string(),
        fields: set.child_fields(),
        frame,
        members,
    })
}
