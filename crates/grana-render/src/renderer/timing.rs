use super::*;
use glam::Mat4;

pub(super) struct Begin<'a> {
    pub(super) target: &'a mut FrameTarget,
    pub(super) levels: LevelKey,
    pub(super) structure: u64,
    pub(super) view: u64,
    pub(super) camera: Mat4,
    pub(super) exporting: bool,
    pub(super) assured: bool,
    pub(super) fade: &'a mut Option<Fade>,
}

#[derive(Clone, Copy)]
pub(super) struct Started {
    pub(super) reset: bool,
    pub(super) moved: bool,
    pub(super) changed: bool,
    pub(super) snapshot: bool,
    pub(super) phase: u32,
}

pub(super) fn begin(begin: Begin<'_>) -> Started {
    let Begin {
        target,
        levels,
        structure,
        view,
        camera,
        exporting,
        assured,
        fade,
    } = begin;
    let moved = target.camera.is_some_and(|held| held != camera);
    let changed = target.view != Some(view);
    if moved || changed {
        *fade = None;
    }
    let snapshot = target.levels.is_some_and(|previous| previous != levels)
        && !moved
        && !exporting
        && !assured
        && target.camera.is_some();
    target.levels = Some(levels);
    let reset = target.structure != Some(structure);
    target.structure = Some(structure);
    target.view = Some(view);
    target.camera = Some(camera);
    if reset {
        target.taa.invalidate();
        target.phase = 0;
    }
    Started {
        reset,
        moved,
        changed,
        snapshot,
        phase: target.phase,
    }
}

pub(super) fn fade(fade: &mut Option<Fade>) -> Option<f32> {
    fade.as_mut().map(|active| {
        active.weight = (active.weight + 1.0 / FADE_DRAWS).min(1.0);
        active.weight
    })
}
