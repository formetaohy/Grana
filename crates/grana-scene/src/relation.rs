use std::sync::Arc;

use grana_object::RelationFrame;

use crate::object_set::ObjectSetId;

#[derive(Debug, PartialEq)]
pub struct Relation {
    target: ObjectSetId,
    shift: i32,
    frames: Vec<Option<Arc<RelationFrame>>>,
}

impl Relation {
    pub(crate) fn new(
        target: ObjectSetId,
        shift: i32,
        frames: Vec<Option<Arc<RelationFrame>>>,
    ) -> Self {
        assert!(
            !frames.is_empty(),
            "a relation covers at least one frame of its dataset"
        );
        assert!(
            (i64::from(shift).unsigned_abs() as usize) < frames.len(),
            "a relation shifts within the frames of its dataset"
        );
        Self {
            target,
            shift,
            frames,
        }
    }

    pub fn target(&self) -> ObjectSetId {
        self.target
    }

    pub fn shift(&self) -> i32 {
        self.shift
    }

    pub fn frame(&self, time: u32) -> Option<&RelationFrame> {
        self.frames
            .get(time as usize)
            .and_then(|frame| frame.as_deref())
    }

    pub fn computed(&self) -> usize {
        self.frames.iter().filter(|frame| frame.is_some()).count()
    }

    pub(crate) fn shared_frames(&self) -> &[Option<Arc<RelationFrame>>] {
        &self.frames
    }

    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    pub(crate) fn source_of(&self, target_time: u32) -> Option<u32> {
        let source = i64::from(target_time) - i64::from(self.shift);
        (source >= 0 && (source as usize) < self.frames.len()).then_some(source as u32)
    }

    pub(crate) fn install(&mut self, time: u32, frame: RelationFrame) -> bool {
        let Some(slot) = self.frames.get_mut(time as usize) else {
            return false;
        };
        *slot = Some(Arc::new(frame));
        true
    }

    pub(crate) fn invalidate(&mut self, time: u32) -> bool {
        let Some(slot) = self.frames.get_mut(time as usize) else {
            return false;
        };
        slot.take().is_some()
    }

    pub(crate) fn shift_to(&mut self, shift: i32) -> bool {
        assert!(
            (i64::from(shift).unsigned_abs() as usize) < self.frames.len(),
            "a relation shifts within the frames of its dataset"
        );
        if self.shift == shift {
            return false;
        }
        self.shift = shift;
        self.frames.iter_mut().for_each(|frame| *frame = None);
        true
    }
}
