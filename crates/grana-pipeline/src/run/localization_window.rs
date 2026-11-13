use std::sync::Arc;

use grana_scene::{ObjectFrame, ObjectSet};

pub struct LocalizationWindow {
    frames: Vec<Option<Arc<ObjectFrame>>>,
    visible: Vec<Option<Vec<bool>>>,
}

impl LocalizationWindow {
    pub fn new(frames: Vec<Option<Arc<ObjectFrame>>>, visible: Vec<Option<Vec<bool>>>) -> Self {
        assert!(
            !frames.is_empty(),
            "a localization window names the frames of its range"
        );
        assert_eq!(
            frames.len(),
            visible.len(),
            "a localization window marks every frame it ranges over"
        );
        Self { frames, visible }
    }

    pub fn of(set: &ObjectSet, frames: [u32; 2]) -> Self {
        assert!(
            frames[0] <= frames[1] && frames[1] < set.frame_count(),
            "a localization window ranges over the frames of its object set"
        );
        let held = (frames[0]..=frames[1])
            .map(|time| set.frame_shared(time))
            .collect::<Vec<_>>();
        let masked = set.filtered() || set.hidden_count() > 0;
        let visible = (frames[0]..=frames[1])
            .map(|time| {
                if !masked {
                    return None;
                }
                set.frame(time).map(|frame| {
                    (0..frame.object_count())
                        .map(|index| set.visible_object(time, index))
                        .collect::<Vec<_>>()
                })
            })
            .collect::<Vec<_>>();
        Self::new(held, visible)
    }

    pub fn len(&self) -> usize {
        self.frames.len()
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    pub fn frame(&self, index: usize) -> Option<&Arc<ObjectFrame>> {
        self.frames.get(index).and_then(Option::as_ref)
    }

    pub fn visible(&self, index: usize) -> Option<&[bool]> {
        self.visible.get(index).and_then(Option::as_deref)
    }
}
