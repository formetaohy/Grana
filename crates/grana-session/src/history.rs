use std::time::{Duration, Instant};

use grana_animation::Timeline;
use grana_display::CameraState;
use grana_project::{Export, NamedView};
use grana_scene::{Measure, MeasureState, Scene, SceneState};
use grana_volume::Pyramid;

use crate::command::{Action, Edit};

const MAX_ENTRIES: usize = 64;
const COALESCE: Duration = Duration::from_millis(600);

pub(crate) struct Snapshot {
    pub(crate) scene: SceneState,
    pub(crate) measure: MeasureState,
    pub(crate) timeline: Timeline,
    pub(crate) export: Export,
    pub(crate) views: Vec<NamedView>,
    pub(crate) camera: Option<CameraState>,
    pub(crate) pyramid: Pyramid,
}

impl Snapshot {
    pub(crate) fn capture(
        scene: &Scene,
        measure: &Measure,
        timeline: &Timeline,
        export: Export,
        views: &[NamedView],
        camera: Option<CameraState>,
    ) -> Self {
        Self {
            scene: scene.state(),
            measure: measure.state(scene),
            timeline: timeline.clone(),
            export,
            views: views.to_vec(),
            camera,
            pyramid: scene.dataset.pyramid().clone(),
        }
    }
}

struct Entry {
    edit: Edit,
    snapshot: Snapshot,
}

pub(crate) enum Undo {
    Coalesced,
    Snapshot(Box<Snapshot>),
}

#[derive(Default)]
pub(crate) struct History {
    undo: Vec<Entry>,
    redo: Vec<Entry>,
    last: Option<(Edit, Instant)>,
}

impl History {
    pub(crate) fn coalesces(&self, edit: &Edit) -> bool {
        self.last.is_some_and(|(last, at)| {
            last.action() == edit.action()
                && last.target() == edit.target()
                && edit.continuous()
                && Instant::now().duration_since(at) <= COALESCE
        })
    }

    pub(crate) fn record(&mut self, edit: Edit, undo: Undo) {
        self.redo.clear();
        self.last = Some((edit, Instant::now()));
        let Undo::Snapshot(snapshot) = undo else {
            return;
        };
        self.undo.push(Entry {
            edit,
            snapshot: *snapshot,
        });
        if self.undo.len() > MAX_ENTRIES {
            self.undo.remove(0);
        }
    }

    pub(crate) fn undo(&mut self, current: Snapshot) -> Option<(Edit, Snapshot)> {
        let entry = self.undo.pop()?;
        self.redo.push(Entry {
            edit: entry.edit,
            snapshot: current,
        });
        self.last = None;
        Some((entry.edit, entry.snapshot))
    }

    pub(crate) fn redo(&mut self, current: Snapshot) -> Option<(Edit, Snapshot)> {
        let entry = self.redo.pop()?;
        self.undo.push(Entry {
            edit: entry.edit,
            snapshot: current,
        });
        self.last = None;
        Some((entry.edit, entry.snapshot))
    }

    pub(crate) fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.last = None;
    }

    pub(crate) fn undo_action(&self) -> Option<Action> {
        self.undo.last().map(|entry| entry.edit.action())
    }

    pub(crate) fn redo_action(&self) -> Option<Action> {
        self.redo.last().map(|entry| entry.edit.action())
    }
}
