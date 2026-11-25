use std::collections::BTreeMap;

use crate::graph::{Graph, Semantic};
use crate::matcher::Matching;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Weight {
    missing: f64,
    extra: f64,
    dropped: f64,
    added: f64,
    altered: f64,
}

impl Weight {
    pub const CTC: Self = Self {
        missing: 10.0,
        extra: 1.0,
        dropped: 1.0,
        added: 1.5,
        altered: 1.0,
    };

    pub fn new(missing: f64, extra: f64, dropped: f64, added: f64, altered: f64) -> Self {
        assert!(
            [missing, extra, dropped, added, altered]
                .into_iter()
                .all(|value| value.is_finite() && value > 0.0),
            "an operation cost is a positive weight"
        );
        Self {
            missing,
            extra,
            dropped,
            added,
            altered,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cost {
    missing: usize,
    extra: usize,
    dropped: usize,
    added: usize,
    altered: usize,
    vertices: usize,
    edges: usize,
}

impl Cost {
    pub fn of(truth: &Graph, computed: &Graph, matching: &Matching) -> Self {
        assert!(
            !truth.is_empty(),
            "an evaluation compares against reference vertices"
        );
        let missing = (0..truth.len())
            .filter(|index| matching.computed_of_truth(*index).is_none())
            .count();
        let extra = (0..computed.len())
            .filter(|index| matching.truth_of_computed(*index).is_none())
            .count();
        let reference = index(truth);
        let held = index(computed);
        let mut added = 0;
        let mut altered = 0;
        let mut linked = BTreeMap::new();
        for edge in truth.edges() {
            let (Some(source), Some(target)) = (
                matching.computed_of_truth(edge.source()),
                matching.computed_of_truth(edge.target()),
            ) else {
                continue;
            };
            linked.insert((source, target), edge.semantic());
        }
        for ((source, target), semantic) in &linked {
            match held.get(&(*source, *target)) {
                Some(found) if found == semantic => {}
                Some(_) => altered += 1,
                None => added += 1,
            }
        }
        let mut dropped = 0;
        for edge in computed.edges() {
            let (Some(source), Some(target)) = (
                matching.truth_of_computed(edge.source()),
                matching.truth_of_computed(edge.target()),
            ) else {
                continue;
            };
            if reference.contains_key(&(source, target)) {
                continue;
            }
            dropped += 1;
        }
        Self {
            missing,
            extra,
            dropped,
            added,
            altered,
            vertices: truth.len(),
            edges: truth.edges().len(),
        }
    }

    pub fn missing(&self) -> usize {
        self.missing
    }

    pub fn extra(&self) -> usize {
        self.extra
    }

    pub fn dropped(&self) -> usize {
        self.dropped
    }

    pub fn added(&self) -> usize {
        self.added
    }

    pub fn altered(&self) -> usize {
        self.altered
    }

    pub fn vertices(&self) -> usize {
        self.vertices
    }

    pub fn edges(&self) -> usize {
        self.edges
    }

    pub fn detection(&self, weight: Weight) -> f64 {
        let scored = weight.missing * self.missing as f64 + weight.extra * self.extra as f64;
        rescale(scored, weight.missing * self.vertices as f64)
    }

    pub fn tracking(&self, weight: Weight) -> f64 {
        rescale(self.aogm(weight), self.scratch(weight))
    }

    pub fn linkage(&self, weight: Weight) -> f64 {
        rescale(self.linkage_cost(weight), weight.added * self.edges as f64)
    }

    pub fn aogm(&self, weight: Weight) -> f64 {
        self.linkage_cost(weight)
            + weight.missing * self.missing as f64
            + weight.extra * self.extra as f64
    }

    pub fn scratch(&self, weight: Weight) -> f64 {
        weight.missing * self.vertices as f64 + weight.added * self.edges as f64
    }

    fn linkage_cost(&self, weight: Weight) -> f64 {
        weight.dropped * self.dropped as f64
            + weight.added * self.added as f64
            + weight.altered * self.altered as f64
    }
}

fn rescale(scored: f64, scratch: f64) -> f64 {
    assert!(
        scratch > 0.0,
        "a score divides a reference that holds cells"
    );
    1.0 - scored.min(scratch) / scratch
}

fn index(graph: &Graph) -> BTreeMap<(usize, usize), Semantic> {
    let mut held = BTreeMap::new();
    for edge in graph.edges() {
        held.insert((edge.source(), edge.target()), edge.semantic());
    }
    held
}
