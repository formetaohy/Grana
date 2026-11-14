use std::sync::Arc;

use grana_analysis::localization::{LocalizationFrame, LocalizationSource};
use grana_object::{LocalizationFilter, Spot};
use grana_scene::{Frame, ObjectSet};

pub struct LocalizationWindow {
    frames: Vec<Option<Frame>>,
    visible: Vec<Option<Arc<Vec<bool>>>>,
    empty: Arc<[Spot]>,
}

impl LocalizationWindow {
    pub fn new(frames: Vec<Option<Frame>>, visible: Vec<Option<Arc<Vec<bool>>>>) -> Self {
        assert!(
            !frames.is_empty(),
            "a localization window names the frames of its range"
        );
        assert_eq!(
            frames.len(),
            visible.len(),
            "a localization window marks every frame it ranges over"
        );
        Self {
            frames,
            visible,
            empty: Vec::new().into(),
        }
    }

    pub fn of(set: &ObjectSet, frames: [u32; 2]) -> Self {
        assert!(
            frames[0] <= frames[1] && frames[1] < set.frame_count(),
            "a localization window ranges over the frames of its object set"
        );
        let held = (frames[0]..=frames[1])
            .map(|time| set.frame_handle(time))
            .collect::<Vec<_>>();
        let masked = set.restricted();
        let visible = (frames[0]..=frames[1])
            .map(|time| {
                if !masked {
                    return None;
                }
                Some(Arc::new(
                    (0..set.object_count(time))
                        .map(|index| {
                            set.filtered_object(time, index) && !set.hidden_object(time, index)
                        })
                        .collect::<Vec<_>>(),
                ))
            })
            .collect::<Vec<_>>();
        Self::new(held, visible)
    }
}

impl LocalizationSource for LocalizationWindow {
    fn len(&self) -> usize {
        self.frames.len()
    }

    fn frame(&self, index: usize, filter: LocalizationFilter) -> LocalizationFrame {
        let spots = self
            .frames
            .get(index)
            .and_then(Option::as_ref)
            .and_then(Frame::spot_frame)
            .map_or_else(|| Arc::clone(&self.empty), |set| set.shared_spots());
        LocalizationFrame::new(
            spots,
            self.visible.get(index).and_then(Option::clone),
            filter,
        )
    }
}
