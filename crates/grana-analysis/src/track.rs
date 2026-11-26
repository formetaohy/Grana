use std::sync::atomic::AtomicBool;

use grana_object::{TrackDetection, TrackParams, TrackSet};

mod calibration;
mod candidate;
mod chain;
mod detection;
mod packing;
mod set;
mod solve;

pub struct TrackLinker {
    params: TrackParams,
}

impl TrackLinker {
    pub fn new(params: TrackParams) -> Self {
        Self { params }
    }

    pub fn link(
        &self,
        frames: &[&[TrackDetection]],
        frame_interval: f32,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(usize, usize),
    ) -> Option<TrackSet> {
        let detections = detection::Detections::new(frames);
        if detections.is_empty() {
            return Some(
                TrackSet::try_with_links(frame_interval, Vec::new(), Vec::new())
                    .expect("an empty track set is valid"),
            );
        }
        let chains = solve::solve(&detections, self.params, cancel, progress)?;
        Some(set::build(&chains, &detections, frame_interval))
    }
}
