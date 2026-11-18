use super::*;
use glam::Mat4;

pub(super) struct Begin<'a> {
    pub(super) target: &'a mut FrameTarget,
    pub(super) levels: LevelKey,
    pub(super) layers: u64,
    pub(super) classification: u64,
    pub(super) appearance: u64,
    pub(super) view: u64,
    pub(super) camera: Mat4,
    pub(super) exporting: bool,
    pub(super) assured: bool,
    pub(super) fade: &'a mut Option<Fade>,
}

#[derive(Clone, Copy)]
pub(super) struct Started {
    pub(super) reset: bool,
    pub(super) restaged: bool,
    pub(super) restyled: bool,
    pub(super) moved: bool,
    pub(super) changed: bool,
    pub(super) snapshot: bool,
    pub(super) phase: u32,
    pub(super) previous: Option<Mat4>,
}

impl Started {
    pub(super) fn clamps_history(self) -> bool {
        self.moved || self.restaged || self.restyled
    }

    pub(super) fn resets_age(self) -> bool {
        self.reset || self.restaged || self.restyled || self.changed
    }
}

pub(super) fn begin(begin: Begin<'_>) -> Started {
    let Begin {
        target,
        levels,
        layers,
        classification,
        appearance,
        view,
        camera,
        exporting,
        assured,
        fade,
    } = begin;
    let previous = target.camera;
    let moved = previous.is_some_and(|held| held != camera);
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
    let reset = target.layers != Some(layers);
    let restaged = target.classification != Some(classification);
    let restyled = target.appearance != Some(appearance);
    target.layers = Some(layers);
    target.classification = Some(classification);
    target.appearance = Some(appearance);
    target.view = Some(view);
    target.camera = Some(camera);
    if reset {
        target.taa.invalidate();
        target.phase = 0;
    }
    Started {
        reset,
        restaged,
        restyled,
        moved,
        changed,
        snapshot,
        phase: target.phase,
        previous,
    }
}

pub(super) fn fade(fade: &mut Option<Fade>) -> Option<f32> {
    fade.as_mut().map(|active| {
        active.weight = (active.weight + 1.0 / FADE_DRAWS).min(1.0);
        active.weight
    })
}
