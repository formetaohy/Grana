use grana_measure::{Aggregate, RelationField, TrackMeasurement, Unit};
use grana_object::{RelationFrame, TrackSet};

use crate::object_set::ObjectSetId;
use crate::relation::Relation;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrackColor {
    Measurement(TrackMeasurement),
    Relation {
        target: ObjectSetId,
        field: RelationField,
        aggregate: Aggregate,
        shift: i32,
    },
}

impl TrackColor {
    pub fn unit(self) -> Unit {
        match self {
            Self::Measurement(measurement) => measurement.unit(),
            Self::Relation { field, .. } => field.unit(),
        }
    }

    pub fn decimals(self) -> u8 {
        match self {
            Self::Measurement(measurement) => measurement.decimals(),
            Self::Relation { field, .. } => field.decimals(),
        }
    }

    pub fn relation(self) -> Option<(ObjectSetId, i32)> {
        match self {
            Self::Measurement(_) => None,
            Self::Relation { target, shift, .. } => Some((target, shift)),
        }
    }

    pub fn target(self) -> Option<ObjectSetId> {
        self.relation().map(|(target, _)| target)
    }

    pub fn value(self, track: usize, set: &TrackSet, relation: Option<&Relation>) -> f32 {
        match self {
            Self::Measurement(measurement) => measurement.value(track, set),
            Self::Relation {
                field, aggregate, ..
            } => {
                let (Some(entry), Some(relation)) = (set.track(track), relation) else {
                    return f32::NAN;
                };
                let mut values = Vec::with_capacity(entry.nodes().len());
                for node in entry.nodes() {
                    values.push(relation_value(
                        field,
                        relation.frame(node.time()),
                        node.object(),
                    ));
                }
                aggregate.apply(&values)
            }
        }
    }
}

pub(crate) fn relation_value(
    field: RelationField,
    frame: Option<&RelationFrame>,
    object: usize,
) -> f32 {
    match frame {
        Some(frame) if object < frame.objects() => field.measure(frame, object),
        _ => f32::NAN,
    }
}
