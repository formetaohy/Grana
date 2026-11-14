use std::sync::atomic::{AtomicBool, Ordering};

use grana_analysis::{LocalizationFrames, LocalizationPoint};
use grana_scene::{Drift, ObjectSource};
use grana_warp::Warp;

use crate::event::Event;

pub struct LocalizationDriftRequest {
    source: LocalizationCloud,
    reference: u32,
    channel: usize,
    base: Drift,
}

struct LocalizationCloud(ObjectSource);

impl LocalizationFrames for LocalizationCloud {
    fn frames(&self) -> usize {
        self.0.frames() as usize
    }

    fn total(&self) -> usize {
        if self.0.filtered() {
            (0..self.0.frames())
                .map(|time| self.points(time).len())
                .sum()
        } else {
            (0..self.0.frames())
                .map(|time| self.0.object_count(time))
                .sum()
        }
    }

    fn points(&self, time: u32) -> Vec<LocalizationPoint> {
        let Some(spots) = self.0.spot_frame(time) else {
            return Vec::new();
        };
        spots
            .spots()
            .iter()
            .enumerate()
            .filter(|(index, spot)| self.0.visible_spot(time, *index, spot))
            .map(|(_, spot)| LocalizationPoint::new(spot.center(), spot.sigma()))
            .collect()
    }
}

impl LocalizationDriftRequest {
    pub fn new(source: ObjectSource, reference: u32, channel: usize, base: Drift) -> Self {
        assert!(
            source.frames() > 0,
            "a localization drift names the frames of its object set"
        );
        assert!(
            base.is_empty() || base.reference() == reference,
            "a localization drift composes with a drift of its own reference"
        );
        Self {
            source: LocalizationCloud(source),
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
        source,
        reference,
        channel,
        base,
    } = request;
    let frames = source.frames();
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
    let Some(found) = grana_analysis::localization_drift(&source, reference, &mut report)? else {
        sink(Event::Done);
        return Ok(());
    };
    let offsets = found.offsets();
    if cancel.load(Ordering::Relaxed) {
        sink(Event::Done);
        return Ok(());
    }
    let mut warps = Vec::with_capacity(frames);
    for (time, offset) in offsets.iter().enumerate() {
        let residual = Warp::shift(*offset);
        warps.push(base.warp(time as u32).added(&residual));
    }
    sink(Event::DriftSummary {
        segments: found.segments(),
        fallback: found.fallback(),
    });
    sink(Event::Drift(Drift::new(channel, reference, warps)));
    sink(Event::Done);
    Ok(())
}
