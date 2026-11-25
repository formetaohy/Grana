use super::*;
use glam::Mat4;

pub(super) struct Begin<'a> {
    pub(super) target: &'a mut FrameTarget,
    pub(super) levels: LevelKey,
    pub(super) layers: u64,
    pub(super) content: u64,
    pub(super) classification: u64,
    pub(super) appearance: u64,
    pub(super) splats: u64,
    pub(super) detail: u64,
    pub(super) lens: u64,
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
    pub(super) resplatted: bool,
    pub(super) moved: bool,
    pub(super) changed: bool,
    pub(super) snapshot: bool,
}

impl Started {
    pub(super) fn clamps_history(self) -> bool {
        self.moved
    }

    pub(super) fn recompacts(self) -> bool {
        self.restart || self.restaged || self.restyled || self.resplatted || self.moved
    }

    pub(super) fn resets_age(self) -> bool {
        self.refresh || self.changed || self.moved
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
        splats,
        detail,
        lens,
        camera,
        exporting,
        assured,
        fade,
    } = begin;
    let moved = target.camera.is_some_and(|held| held != camera);
    let changed = target.lens != Some(lens);
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
    let resplatted = target.splats != Some(splats);
    let redetailed = target.detail != Some(detail);
    let refresh = restart
        || target.content != Some(content)
        || restaged
        || restyled
        || resplatted
        || redetailed;
    target.layers = Some(layers);
    target.content = Some(content);
    target.classification = Some(classification);
    target.appearance = Some(appearance);
    target.splats = Some(splats);
    target.detail = Some(detail);
    target.lens = Some(lens);
    target.camera = Some(camera);
    if refresh || changed {
        target.temporal.invalidate();
        target.phase = 0;
    }
    Started {
        restart,
        refresh,
        restaged,
        restyled,
        resplatted,
        moved,
        changed,
        snapshot,
    }
}

pub(super) fn fade(fade: &mut Option<Fade>) -> Option<f32> {
    let active = fade.as_mut()?;
    active.weight += 1.0 / FADE_DRAWS;
    if active.weight >= 1.0 {
        *fade = None;
        return None;
    }
    Some(active.weight)
}
