use std::sync::Arc;

use grana_object::RelationFrame;

use crate::object_set::ObjectSetId;

#[derive(Debug, PartialEq)]
pub struct Relation {
    target: ObjectSetId,
    shift: i32,
    key: String,
    frames: Vec<Option<Arc<RelationFrame>>>,
}

pub(crate) fn key_of(target: ObjectSetId, shift: i32) -> String {
    format!("r{}_{}", target.raw(), shift)
}

impl Relation {
    pub(crate) fn new(
        target: ObjectSetId,
        shift: i32,
        key: String,
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
        assert!(!key.trim().is_empty(), "a relation carries a stable key");
        Self {
            target,
            shift,
            key,
            frames,
        }
    }

    pub fn target(&self) -> ObjectSetId {
        self.target
    }

    pub fn shift(&self) -> i32 {
        self.shift
    }

    pub fn key(&self) -> &str {
        &self.key
    }

    pub fn frame(&self, time: u32) -> Option<&RelationFrame> {
        self.frames
            .get(time as usize)
            .and_then(|frame| frame.as_deref())
    }

    pub(crate) fn frame_shared(&self, time: u32) -> Option<Arc<RelationFrame>> {
        self.frames.get(time as usize).and_then(Option::clone)
    }

    pub fn frames(&self) -> impl Iterator<Item = (u32, &RelationFrame)> + '_ {
        self.frames
            .iter()
            .enumerate()
            .filter_map(|(time, frame)| frame.as_deref().map(|frame| (time as u32, frame)))
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

    pub(crate) fn moved(&self, shift: i32) -> Self {
        Self::new(
            self.target,
            shift,
            key_of(self.target, shift),
            vec![None; self.frames.len()],
        )
    }
}
