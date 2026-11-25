mod branch;
mod cost;
mod graph;
mod marker;
mod matcher;
mod phantom;
mod truth;

pub use branch::Branch;
pub use cost::{Cost, Weight};
pub use graph::{Division, Edge, Graph, Semantic, Vertex};
pub use marker::{Marker, TrackRow};
pub use matcher::Matching;
pub use phantom::{Phantom, Spec};
pub use truth::Truth;

use grana_object::TrackSet;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Evaluation {
    cost: Cost,
    divisions: Branch,
    weight: Weight,
}

impl Evaluation {
    pub fn of(truth: &Truth, computed: &TrackSet, tolerance: f32) -> Self {
        let graph = Graph::of_tracks(computed);
        let matching = Matching::of(truth, &graph, tolerance);
        Self {
            cost: Cost::of(truth.graph(), &graph, &matching),
            divisions: Branch::of(truth.graph(), &graph, &matching),
            weight: Weight::CTC,
        }
    }

    pub fn detection(&self) -> f64 {
        self.cost.detection(self.weight)
    }

    pub fn tracking(&self) -> f64 {
        self.cost.tracking(self.weight)
    }

    pub fn linkage(&self) -> f64 {
        self.cost.linkage(self.weight)
    }

    pub fn divisions(&self) -> Branch {
        self.divisions
    }

    pub fn cost(&self) -> Cost {
        self.cost
    }
}
