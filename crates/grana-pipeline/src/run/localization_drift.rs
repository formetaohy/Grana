use std::sync::atomic::{AtomicBool, Ordering};

use grana_analysis::LocalizationPoint;
use grana_scene::Drift;
use grana_warp::Warp;

use crate::event::Event;

pub struct LocalizationDriftRequest {
    frames: Vec<Vec<LocalizationPoint>>,
    reference: u32,
    channel: usize,
    base: Drift,
}

impl LocalizationDriftRequest {
    pub fn new(
        frames: Vec<Vec<LocalizationPoint>>,
        reference: u32,
        channel: usize,
        base: Drift,
    ) -> Self {
        assert!(
            !frames.is_empty(),
            "a localization drift names the frames of its object set"
        );
        assert!(
            base.is_empty() || base.reference() == reference,
            "a localization drift composes with a drift of its own reference"
        );
        Self {
            frames,
            reference,
            channel,
            base,
        }
    }
}

pub fn localization_drift(
    request: LocalizationDriftRequest,
    cancel: &AtomicBool,
    sink: &mut dyn FnMut(Event),
) -> Result<(), String> {
    let LocalizationDriftRequest {
        frames,
        reference,
        channel,
        base,
    } = request;
    sink(Event::Progress {
        time: 0,
        done: 0,
        total: 0,
    });
    let mut report = |done: usize, total: usize| {
        sink(Event::Progress {
            time: 0,
            done,
            total,
        });
        !cancel.load(Ordering::Relaxed)
    };
    let Some(offsets) = grana_analysis::localization_drift(&frames, reference, &mut report)? else {
        sink(Event::Done);
        return Ok(());
    };
    if cancel.load(Ordering::Relaxed) {
        sink(Event::Done);
        return Ok(());
    }
    let mut warps = Vec::with_capacity(frames.len());
    for (time, offset) in offsets.iter().enumerate() {
        let residual = Warp::shift(*offset);
        warps.push(base.warp(time as u32).added(&residual));
    }
    sink(Event::Drift(Drift::new(channel, reference, warps)));
    sink(Event::Done);
    Ok(())
}
