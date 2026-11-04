use grana_display::Extent;
use grana_measure::{Measurement, Objects, RelationField, Unit};
use grana_object::RelationFrame;

use crate::object::ObjectFrame;
use crate::object_set::ObjectSetId;
use crate::track_color::relation_value;

pub const RELATION_FIELDS: [RelationField; 3] = [
    RelationField::Distance,
    RelationField::Inside,
    RelationField::Contact,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ObjectColor {
    Measurement(Measurement),
    Relation {
        target: ObjectSetId,
        field: RelationField,
        shift: i32,
    },
}

impl ObjectColor {
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

    pub fn value(
        self,
        objects: Objects<'_>,
        relation: Option<&RelationFrame>,
        object: usize,
    ) -> f32 {
        match self {
            Self::Measurement(measurement) => measurement.value(objects, object),
            Self::Relation { field, .. } => relation_value(field, relation, object),
        }
    }

    pub(crate) fn extent(
        self,
        frame: &ObjectFrame,
        relation: Option<&RelationFrame>,
    ) -> Option<Extent> {
        let objects = frame.objects();
        (0..frame.object_count()).fold(None, |accumulated, object| {
            let value = self.value(objects, relation, object);
            if !value.is_finite() {
                return accumulated;
            }
            Some(match accumulated {
                None => (value, value),
                Some((minimum, maximum)) => (minimum.min(value), maximum.max(value)),
            })
        })
    }
}
