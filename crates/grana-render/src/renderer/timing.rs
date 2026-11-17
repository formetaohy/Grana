use std::time::{Duration, Instant};

use super::*;
use crate::converge::Sampling;
use crate::temporal::Temporal;

pub(super) struct Begin<'a> {
    pub(super) target: &'a mut FrameTarget,
    pub(super) sampling: Sampling,
    pub(super) budget: Option<Duration>,
    pub(super) levels: LevelKey,
    pub(super) transition: u64,
    pub(super) structure: u64,
    pub(super) shot: u64,
    pub(super) temporal: Option<Temporal>,
    pub(super) sequence: u64,
    pub(super) fade: &'a mut Option<Fade>,
    pub(super) exporting: bool,
    pub(super) assured: bool,
}

#[derive(Clone, Copy)]
pub(super) struct Started {
    pub(super) reset: bool,
    pub(super) snapshot: bool,
    pub(super) continuing: bool,
    pub(super) expired: bool,
    pub(super) sample: u32,
}

pub(super) fn begin(begin: Begin<'_>) -> Started {
    let Begin {
        target,
        sampling,
        budget,
        levels,
        transition,
        structure,
        shot,
        temporal,
        sequence,
        fade,
        exporting,
        assured,
    } = begin;
    let moved = target.transition != Some(transition);
    if moved {
        *fade = None;
    }
    let snapshot = target.levels.is_some_and(|previous| previous != levels)
        && !moved
        && !exporting
        && !assured
        && sampling.ceiling() > 1
        && target.samples > 0;
    target.transition = Some(transition);
    target.levels = Some(levels);
    if target.signature != Some(structure) {
        target.signature = Some(structure);
        target.samples = 0;
    }
    let reset = target.samples == 0;
    if reset {
        target.deadline = budget.and_then(|budget| Instant::now().checked_add(budget));
        target.estimate = None;
    }
    let expired = target
        .deadline
        .is_some_and(|deadline| deadline <= Instant::now());
    let mut continuing = false;
    if reset {
        target.seeded = false;
        continuing = temporal.is_some() && target.holds && target.shot == Some(shot);
        if !continuing {
            target.holds = false;
        }
        target.shot = Some(shot);
    }
    let sample = if reset && !assured && temporal.is_some() {
        (sequence % u64::from(sampling.ceiling())) as u32
    } else {
        target.samples.min(sampling.ceiling() - 1)
    };
    Started {
        reset,
        snapshot,
        continuing,
        expired,
        sample,
    }
}

pub(super) fn fade(fade: &mut Option<Fade>) -> Option<f32> {
    fade.as_mut().map(|active| {
        active.weight = (active.weight + 1.0 / FADE_DRAWS).min(1.0);
        active.weight
    })
}
