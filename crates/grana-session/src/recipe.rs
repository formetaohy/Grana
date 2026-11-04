use std::collections::HashSet;

use grana_measure::{ColocPair, ColocSettings, ColocThresholds};
use grana_scene::{Job, ObjectSetId};

use grana_protocol::Step;

pub(crate) struct Recipe {
    steps: Vec<Step>,
    sets: Vec<ObjectSetId>,
    parts: HashSet<ObjectSetId>,
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
        let parts = steps
            .iter()
            .filter_map(|step| match step {
                Step::Merge { part, .. } => Some(*part),
                Step::Detect { .. }
                | Step::Filter { .. }
                | Step::Copy { .. }
                | Step::Relation { .. }
                | Step::Coloc { .. }
                | Step::Present => None,
            })
            .collect();
        Self {
            steps,
            sets,
            parts,
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

    pub(crate) fn step(&self) -> &Step {
        &self.steps[self.step]
    }

    pub(crate) fn advance(&mut self) {
        self.step = (self.step + 1).min(self.steps.len() - 1);
    }

    pub(crate) fn pair(&self, index: usize) -> (ColocPair, ColocThresholds) {
        self.pairs[index]
    }

    pub(crate) fn part(&self, set: ObjectSetId) -> bool {
        self.parts.contains(&set)
    }

    pub(crate) fn merging(&self, set: ObjectSetId) -> bool {
        self.steps[self.step + 1..]
            .iter()
            .any(|step| matches!(step, Step::Merge { set: merged, .. } if *merged == set))
    }

    pub(crate) fn job(&self) -> Option<Job> {
        match self.step() {
            Step::Detect { set, .. } => Some(Job::Detect(*set)),
            Step::Relation { source, target } => Some(Job::Relation {
                source: *source,
                target: *target,
            }),
            Step::Coloc { .. } => Some(Job::Coloc),
            Step::Filter { .. } | Step::Copy { .. } | Step::Merge { .. } | Step::Present => None,
        }
    }
}
