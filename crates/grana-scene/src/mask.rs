use std::sync::Arc;

use grana_measure::RelationField;
use grana_object::{RelationFrame, Roi, Shape};

use crate::object_set::ObjectSetId;
use crate::source::ObjectSource;

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

pub(crate) struct RelationGate {
    frames: Vec<Option<Arc<RelationFrame>>>,
    source: String,
    target: String,
}

impl RelationGate {
    pub(crate) fn new(
        frames: Vec<Option<Arc<RelationFrame>>>,
        source: String,
        target: String,
    ) -> Self {
        Self {
            frames,
            source,
            target,
        }
    }
}

pub struct MaskGate {
    source: ObjectSource,
    filter: Option<MaskFilter>,
    relation: Option<RelationGate>,
}

impl MaskGate {
    pub(crate) fn new(
        source: ObjectSource,
        filter: Option<MaskFilter>,
        relation: Option<RelationGate>,
    ) -> Arc<Self> {
        assert_eq!(
            filter.is_some(),
            relation.is_some(),
            "an analysis mask filter carries the relation it reads"
        );
        Arc::new(Self {
            source,
            filter,
            relation,
        })
    }

    pub fn frames(&self) -> u32 {
        self.source.frames()
    }

    pub fn shape(&self, time: u32) -> Result<Option<Shape>, String> {
        let Some(frame) = self.source.frame(time) else {
            return Ok(None);
        };
        if !self.source.per_object_visibility() && self.filter.is_none() {
            return frame.shape().map(Some).map_err(|error| error.to_string());
        }
        let objects = frame.object_count();
        let mut keep = (0..objects)
            .map(|object| self.source.filtered_object(time, object))
            .collect::<Vec<_>>();
        if let Some(filter) = self.filter
            && keep.iter().any(|kept| *kept)
        {
            let relation = self
                .relation
                .as_ref()
                .expect("an analysis mask filter carries the relation it reads");
            let measured = relation
                .frames
                .get(time as usize)
                .and_then(Option::as_deref)
                .ok_or_else(|| {
                    format!(
                        "the analysis mask needs the relation of {} to {} at frame {time}",
                        relation.source, relation.target
                    )
                })?;
            if measured.objects() != objects {
                return Err(format!(
                    "the relation of {} to {} measures {} objects at frame {time} and the frame holds {objects}",
                    relation.source,
                    relation.target,
                    measured.objects()
                ));
            }
            for (object, kept) in keep.iter_mut().enumerate() {
                *kept &= filter.holds(measured, object);
            }
        }
        let frame = if keep.iter().all(|kept| *kept) {
            frame
        } else {
            let Some(retained) = frame.retain(&keep).map_err(|error| error.to_string())? else {
                return Ok(None);
            };
            Arc::new(retained)
        };
        frame.shape().map(Some).map_err(|error| error.to_string())
    }

    pub fn roi(self: &Arc<Self>) -> Roi {
        let gate = Arc::clone(self);
        let frames = self.frames() as usize;
        Roi::paged(frames, move |time| gate.shape(time))
    }
}
