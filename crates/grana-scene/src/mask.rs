use grana_measure::RelationField;
use grana_object::RelationFrame;

use crate::object_set::ObjectSetId;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MaskFilter {
    target: ObjectSetId,
    shift: i32,
    field: RelationField,
    range: [f32; 2],
}

impl MaskFilter {
    pub fn new(
        target: ObjectSetId,
        shift: i32,
        field: RelationField,
        minimum: f32,
        maximum: f32,
    ) -> Self {
        assert!(
            RelationField::CONTINUOUS.contains(&field),
            "an analysis mask keeps a continuous relation field"
        );
        assert!(
            minimum.is_finite() && maximum.is_finite() && minimum <= maximum,
            "an analysis mask keeps a finite range of relation values"
        );
        Self {
            target,
            shift,
            field,
            range: [minimum, maximum],
        }
    }

    pub fn target(self) -> ObjectSetId {
        self.target
    }

    pub fn shift(self) -> i32 {
        self.shift
    }

    pub fn field(self) -> RelationField {
        self.field
    }

    pub fn range(self) -> [f32; 2] {
        self.range
    }

    pub fn holds(self, frame: &RelationFrame, object: usize) -> bool {
        let value = self.field.measure(frame, object);
        value >= self.range[0] && value <= self.range[1]
    }
}
