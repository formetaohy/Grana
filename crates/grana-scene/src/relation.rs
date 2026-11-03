use std::sync::Arc;

use grana_object::RelationFrame;

use crate::object_set::ObjectSetId;

#[derive(Debug, PartialEq)]
pub struct Relation {
    target: ObjectSetId,
    frames: Vec<Option<Arc<RelationFrame>>>,
}

impl Relation {
    pub(crate) fn new(target: ObjectSetId, frames: Vec<Option<Arc<RelationFrame>>>) -> Self {
        assert!(
            !frames.is_empty(),
            "a relation covers at least one frame of its dataset"
        );
        Self { target, frames }
    }

    pub fn target(&self) -> ObjectSetId {
        self.target
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
}
