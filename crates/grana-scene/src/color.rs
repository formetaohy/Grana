use grana_display::Extent;
use grana_measure::{
    ChildField, ColocMetric, ColocPair, Measurement, Objects, RelationField, Unit,
};
use grana_object::RelationFrame;

use crate::object::ObjectFrame;
use crate::object_set::ObjectSetId;
use crate::track_color::relation_value;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ObjectColor {
    Identity,
    Measurement(Measurement),
    Relation {
        target: ObjectSetId,
        field: RelationField,
        shift: i32,
    },
    Child {
        target: ObjectSetId,
        field: ChildField,
    },
    Coloc {
        pair: ColocPair,
        metric: ColocMetric,
    },
    Score,
}

impl ObjectColor {
    pub fn unit(self) -> Unit {
        match self {
            Self::Identity => Unit::Count,
            Self::Measurement(measurement) => measurement.unit(),
            Self::Relation { field, .. } => field.unit(),
            Self::Child { field, .. } => field.unit(),
            Self::Coloc { .. } | Self::Score => Unit::Count,
        }
    }

    pub fn decimals(self) -> u8 {
        match self {
            Self::Identity => 0,
            Self::Measurement(measurement) => measurement.decimals(),
            Self::Relation { field, .. } => field.decimals(),
            Self::Child { field, .. } => field.decimals(),
            Self::Coloc { .. } => 3,
            Self::Score => 3,
        }
    }

    pub fn relation(self) -> Option<(ObjectSetId, i32)> {
        match self {
            Self::Identity
            | Self::Measurement(_)
            | Self::Child { .. }
            | Self::Coloc { .. }
            | Self::Score => None,
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
            Self::Identity => object as f32,
            Self::Measurement(measurement) => measurement.value(objects, object),
            Self::Relation { field, .. } => relation_value(field, relation, object),
            Self::Child { .. } => {
                panic!("a child aggregate is read from the objects of its parent set")
            }
            Self::Coloc { .. } => {
                panic!("an object colocalization is read from the colocalization of its set")
            }
            Self::Score => panic!("a classifier score is read from the classifier of its set"),
        }
    }

    pub(crate) fn extent(
        self,
        frame: &ObjectFrame,
        relation: Option<&RelationFrame>,
    ) -> Option<Extent> {
        if matches!(self, Self::Score | Self::Coloc { .. }) {
            return None;
        }
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
