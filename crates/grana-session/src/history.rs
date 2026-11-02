use std::time::{Duration, Instant};

use grana_animation::Timeline;
use grana_scene::{Measure, MeasureState, ObjectSetId, Scene, SceneState};

use crate::command::{Action, Edit};

const MAX_ENTRIES: usize = 64;
const COALESCE: Duration = Duration::from_millis(600);

pub(crate) struct Snapshot {
    scene: SceneState,
    measure: MeasureState,
    timeline: Timeline,
}

impl Snapshot {
    pub(crate) fn capture(scene: &Scene, measure: &Measure, timeline: &Timeline) -> Self {
        let mut state = measure.state();
        let set = state
            .set
            .and_then(ObjectSetId::from_raw)
            .and_then(|id| scene.objects.get(id));
        if !set.is_some_and(|set| set.detected() > 0) {
            state.filters.clear();
            state.lasso = None;
            state.sort = None;
            state.columns.clear();
            state.axes = [0; 2];
            if set.is_none() {
                state.set = None;
            }
        }
        Self {
            scene: scene.state(),
            measure: state,
            timeline: timeline.clone(),
        }
    }

    pub(crate) fn into_parts(self) -> (SceneState, MeasureState, Timeline) {
        (self.scene, self.measure, self.timeline)
    }
}

struct Entry {
    edit: Edit,
    snapshot: Snapshot,
}

#[derive(Default)]
pub(crate) struct History {
    undo: Vec<Entry>,
    redo: Vec<Entry>,
    last: Option<(Edit, Instant)>,
}

impl History {
    pub(crate) fn record(&mut self, edit: Edit, snapshot: Snapshot) {
        let now = Instant::now();
        let merged = self.last.is_some_and(|(last, at)| {
            last.action() == edit.action()
                && last.target() == edit.target()
                && edit.continuous()
                && now.duration_since(at) <= COALESCE
        });
        self.redo.clear();
        self.last = Some((edit, now));
        if merged {
            return;
        }
        self.undo.push(Entry { edit, snapshot });
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
