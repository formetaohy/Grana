use std::collections::BTreeMap;

use crate::graph::Graph;
use crate::truth::Truth;

#[derive(Clone, Debug, PartialEq)]
pub struct Matching {
    computed_of_truth: Vec<Option<usize>>,
    truth_of_computed: Vec<Option<usize>>,
}

impl Matching {
    pub fn of(truth: &Truth, computed: &Graph, tolerance: f32) -> Self {
        assert!(
            tolerance.is_finite() && tolerance > 0.0,
            "a marker tolerance is a positive length"
        );
        let graph = truth.graph();
        let mut tracking = BTreeMap::<u32, Vec<usize>>::new();
        for (index, vertex) in graph.vertices().iter().enumerate() {
            tracking.entry(vertex.time()).or_default().push(index);
        }
        let mut found = BTreeMap::<u32, Vec<usize>>::new();
        for (index, vertex) in computed.vertices().iter().enumerate() {
            found.entry(vertex.time()).or_default().push(index);
        }
        let mut computed_of_truth = vec![None; graph.len()];
        let mut truth_of_computed = vec![None; computed.len()];
        for (time, markers) in &tracking {
            let Some(detections) = found.get(time) else {
                continue;
            };
            let mut candidates = Vec::new();
            for marker in markers {
                let reference = graph.vertices()[*marker];
                let radii = truth.radii(*marker);
                for detection in detections {
                    let vertex = computed.vertices()[*detection];
                    let distance = distance(reference.position(), vertex.position(), radii);
                    if distance <= tolerance {
                        candidates.push((distance, *marker, *detection));
                    }
                }
            }
            candidates.sort_unstable_by(|left, right| {
                left.0
                    .total_cmp(&right.0)
                    .then(left.1.cmp(&right.1))
                    .then(left.2.cmp(&right.2))
            });
            for (_, marker, detection) in candidates {
                if computed_of_truth[marker].is_some() || truth_of_computed[detection].is_some() {
                    continue;
                }
                computed_of_truth[marker] = Some(detection);
                truth_of_computed[detection] = Some(marker);
            }
        }
        Self {
            computed_of_truth,
            truth_of_computed,
        }
    }

    pub fn computed_of_truth(&self, index: usize) -> Option<usize> {
        self.computed_of_truth[index]
    }

    pub fn truth_of_computed(&self, index: usize) -> Option<usize> {
        self.truth_of_computed[index]
    }

    pub fn matched(&self) -> usize {
        self.computed_of_truth.iter().flatten().count()
    }
}

fn distance(left: [f32; 3], right: [f32; 3], radii: [f32; 3]) -> f32 {
    (0..3)
        .map(|axis| ((right[axis] - left[axis]) / radii[axis]).powi(2))
        .sum::<f32>()
        .sqrt()
}
