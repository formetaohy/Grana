use grana_measure::{Aggregate, RelationField, Scale, TrackMeasurement, Unit};
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
    pub fn unit(self, scale: Scale) -> Unit {
        match self {
            Self::Measurement(measurement) => measurement.unit(scale),
            Self::Relation { field, .. } => field.unit(scale),
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
                aggregate.apply(&values).present().unwrap_or(f32::NAN)
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
