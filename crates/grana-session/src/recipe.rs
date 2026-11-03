use grana_measure::{ColocPair, ColocSettings, ColocThresholds};
use grana_scene::{Job, ObjectSetId};

#[derive(Clone, Copy)]
pub(crate) enum Step {
    Detect {
        set: ObjectSetId,
        mask: Option<ObjectSetId>,
    },
    Relation {
        source: ObjectSetId,
        target: ObjectSetId,
    },
    Coloc {
        pair: ColocPair,
        thresholds: ColocThresholds,
        mask: Option<ObjectSetId>,
    },
    Present,
}

pub(crate) struct Recipe {
    sets: Vec<ObjectSetId>,
    masks: Vec<Option<ObjectSetId>>,
    relations: Vec<(ObjectSetId, ObjectSetId)>,
    pairs: Vec<(ColocPair, ColocThresholds)>,
    settings: ColocSettings,
    mask: Option<ObjectSetId>,
    step: usize,
}

impl Recipe {
    pub(crate) fn new(
        sets: Vec<ObjectSetId>,
        masks: Vec<Option<ObjectSetId>>,
        relations: Vec<(ObjectSetId, ObjectSetId)>,
        pairs: Vec<(ColocPair, ColocThresholds)>,
        settings: ColocSettings,
        mask: Option<ObjectSetId>,
    ) -> Self {
        assert_eq!(sets.len(), masks.len(), "a recipe masks every object set");
        assert!(!sets.is_empty(), "a recipe detects at least one object set");
        Self {
            sets,
            masks,
            relations,
            pairs,
            settings,
            mask,
            step: 0,
        }
    }

    pub(crate) fn sets(&self) -> &[ObjectSetId] {
        &self.sets
    }

    pub(crate) fn settings(&self) -> ColocSettings {
        self.settings
    }

    pub(crate) fn steps(&self) -> usize {
        self.sets.len() + self.relations.len() + self.pairs.len()
    }

    pub(crate) fn step(&self) -> Step {
        let step = self.step;
        if let Some(set) = self.sets.get(step) {
            return Step::Detect {
                set: *set,
                mask: self.masks[step],
            };
        }
        let step = step - self.sets.len();
        if let Some((source, target)) = self.relations.get(step) {
            return Step::Relation {
                source: *source,
                target: *target,
            };
        }
        let step = step - self.relations.len();
        if let Some((pair, thresholds)) = self.pairs.get(step) {
            return Step::Coloc {
                pair: *pair,
                thresholds: *thresholds,
                mask: self.mask,
            };
        }
        Step::Present
    }

    pub(crate) fn job(&self) -> Option<Job> {
        match self.step() {
            Step::Detect { set, .. } => Some(Job::Detect(set)),
            Step::Relation { source, target } => Some(Job::Relation { source, target }),
            Step::Coloc { .. } => Some(Job::Coloc),
            Step::Present => None,
        }
    }

    pub(crate) fn advance(&mut self) {
        self.step = (self.step + 1).min(self.steps());
    }
}
