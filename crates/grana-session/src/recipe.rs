use grana_measure::{ColocPair, ColocSettings, ColocThresholds};
use grana_scene::{Job, ObjectSetId};

use grana_protocol::Step;

pub(crate) struct Recipe {
    steps: Vec<Step>,
    sets: Vec<ObjectSetId>,
    pairs: Vec<(ColocPair, ColocThresholds)>,
    settings: ColocSettings,
    step: usize,
}

impl Recipe {
    pub(crate) fn new(
        steps: Vec<Step>,
        sets: Vec<ObjectSetId>,
        pairs: Vec<(ColocPair, ColocThresholds)>,
        settings: ColocSettings,
    ) -> Self {
        assert!(
            steps.last() == Some(&Step::Present),
            "a recipe ends by presenting its analysis"
        );
        assert!(!sets.is_empty(), "a recipe detects at least one object set");
        Self {
            steps,
            sets,
            pairs,
            settings,
            step: 0,
        }
    }

    pub(crate) fn sets(&self) -> &[ObjectSetId] {
        &self.sets
    }

    pub(crate) fn settings(&self) -> ColocSettings {
        self.settings
    }

    pub(crate) fn step(&self) -> Step {
        self.steps[self.step]
    }

    pub(crate) fn set(&self, index: usize) -> ObjectSetId {
        self.sets[index]
    }

    pub(crate) fn mask(&self, index: Option<usize>) -> Option<ObjectSetId> {
        index.map(|index| self.sets[index])
    }

    pub(crate) fn pair(&self, index: usize) -> (ColocPair, ColocThresholds) {
        self.pairs[index]
    }

    pub(crate) fn job(&self) -> Option<Job> {
        match self.step() {
            Step::Detect { set, .. } => Some(Job::Detect(self.set(set))),
            Step::Relation { source, target } => Some(Job::Relation {
                source: self.set(source),
                target: self.set(target),
            }),
            Step::Coloc { .. } => Some(Job::Coloc),
            Step::Present => None,
        }
    }

    pub(crate) fn advance(&mut self) {
        self.step = (self.step + 1).min(self.steps.len() - 1);
    }
}
