use std::sync::Arc;

use grana_scene::ObjectFrame;

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
