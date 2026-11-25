use grana_measure::{Aggregate, RelationField, TrackMeasurement, Unit};
use grana_object::{ObjectId, RelationFrame, TrackId, TrackSet};

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

    pub fn value(self, track: TrackId, set: &TrackSet, relation: Option<&Relation>) -> f32 {
        match self {
            Self::Measurement(measurement) => measurement.value(track, set),
            Self::Relation {
                field, aggregate, ..
            } => {
                let (Some(entry), Some(relation)) = (set.track(track), relation) else {
                    return f32::NAN;
                };
                let values = entry
                    .nodes()
                    .iter()
                    .map(|node| relation_value(field, relation.frame(node.time()), Some(node.id())))
                    .collect::<Vec<_>>();
                aggregate.apply(&values).present().unwrap_or(f32::NAN)
            }
        }
    }
}

pub(crate) fn relation_value(
    field: RelationField,
    frame: Option<&RelationFrame>,
    object: Option<ObjectId>,
) -> f32 {
    match (frame, object) {
        (Some(frame), Some(id)) => {
            let value = field.measure(frame, id);
            if value.is_finite() { value } else { f32::NAN }
        }
        _ => f32::NAN,
    }
}
