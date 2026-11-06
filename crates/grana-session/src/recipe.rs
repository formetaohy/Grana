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

    pub(crate) fn step(&self) -> &Step {
        &self.steps[self.step]
    }

    pub(crate) fn advance(&mut self) {
        self.step = (self.step + 1).min(self.steps.len() - 1);
    }

    pub(crate) fn pair(&self, index: usize) -> (ColocPair, ColocThresholds) {
        self.pairs[index]
    }

    pub(crate) fn step_sets(&self) -> Vec<ObjectSetId> {
        match self.step() {
            Step::Drift { .. } => Vec::new(),
            Step::Detect { set, .. }
            | Step::Import { set }
            | Step::Filter { set, .. }
            | Step::Copy { set, .. } => {
                vec![*set]
            }
            Step::Merge { set, part } => vec![*set, *part],
            Step::Relation { source, target, .. } => vec![*source, *target],
            Step::Coloc { .. } | Step::Present => Vec::new(),
        }
    }

    pub(crate) fn matches(&self, job: Job) -> bool {
        self.job()
            .is_some_and(|expected| expected == job || follows(expected, job))
    }

    pub(crate) fn job(&self) -> Option<Job> {
        match self.step() {
            Step::Drift { .. } => Some(Job::Drift),
            Step::Detect { set, .. } => Some(Job::Detect(*set)),
            Step::Import { set } => Some(Job::Import(*set)),
            Step::Relation {
                source,
                target,
                shift,
                ..
            } => Some(Job::Relation {
                source: *source,
                target: *target,
                shift: *shift,
            }),
            Step::Coloc { .. } => Some(Job::Coloc),
            Step::Filter { .. } | Step::Copy { .. } | Step::Merge { .. } | Step::Present => None,
        }
    }
}

fn follows(expected: Job, actual: Job) -> bool {
    match (expected, actual) {
        (Job::Detect(set), Job::Track(track)) | (Job::Recut(set), Job::Track(track)) => {
            set == track
        }
        _ => false,
    }
}
