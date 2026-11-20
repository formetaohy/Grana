use super::*;
use glam::Mat4;

pub(super) struct Begin<'a> {
    pub(super) target: &'a mut FrameTarget,
    pub(super) levels: LevelKey,
    pub(super) layers: u64,
    pub(super) content: u64,
    pub(super) classification: u64,
    pub(super) appearance: u64,
    pub(super) detail: u64,
    pub(super) view: u64,
    pub(super) camera: Mat4,
    pub(super) exporting: bool,
    pub(super) assured: bool,
    pub(super) fade: &'a mut Option<Fade>,
}

#[derive(Clone, Copy)]
pub(super) struct Started {
    pub(super) restart: bool,
    pub(super) refresh: bool,
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
        self.moved
    }

    pub(super) fn recompacts(self) -> bool {
        self.restart || self.restaged || self.restyled || self.moved
    }

    pub(super) fn resets_age(self) -> bool {
        self.refresh || self.changed
    }
}

pub(super) fn begin(begin: Begin<'_>) -> Started {
    let Begin {
        target,
        levels,
        layers,
        content,
        classification,
        appearance,
        detail,
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
    let restart = target.layers != Some(layers);
    let restaged = target.classification != Some(classification);
    let restyled = target.appearance != Some(appearance);
    let redetailed = target.detail != Some(detail);
    let refresh = restart || target.content != Some(content) || restaged || restyled || redetailed;
    target.layers = Some(layers);
    target.content = Some(content);
    target.classification = Some(classification);
    target.appearance = Some(appearance);
    target.detail = Some(detail);
    target.view = Some(view);
    target.camera = Some(camera);
    if refresh {
        target.taa.invalidate();
        target.phase = 0;
    }
    Started {
        restart,
        refresh,
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
