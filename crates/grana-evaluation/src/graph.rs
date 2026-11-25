use std::collections::HashMap;

use grana_object::{Link, TrackSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Semantic {
    Track,
    Parent,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vertex {
    time: u32,
    position: [f32; 3],
}

impl Vertex {
    pub fn new(time: u32, position: [f32; 3]) -> Self {
        assert!(
            position.iter().all(|value| value.is_finite()),
            "a graph vertex names a finite position"
        );
        Self { time, position }
    }

    pub fn time(self) -> u32 {
        self.time
    }

    pub fn position(self) -> [f32; 3] {
        self.position
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Edge {
    source: usize,
    target: usize,
    semantic: Semantic,
}

impl Edge {
    pub fn new(source: usize, target: usize, semantic: Semantic) -> Self {
        assert!(source != target, "a graph edge joins two vertices");
        Self {
            source,
            target,
            semantic,
        }
    }

    pub fn source(self) -> usize {
        self.source
    }

    pub fn target(self) -> usize {
        self.target
    }

    pub fn semantic(self) -> Semantic {
        self.semantic
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Division {
    parent: usize,
    children: [usize; 2],
}

impl Division {
    pub fn parent(self) -> usize {
        self.parent
    }

    pub fn children(self) -> [usize; 2] {
        self.children
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Graph {
    vertices: Vec<Vertex>,
    edges: Vec<Edge>,
}

impl Graph {
    pub fn new(vertices: Vec<Vertex>, mut edges: Vec<Edge>) -> Self {
        for edge in &edges {
            assert!(
                edge.source < vertices.len() && edge.target < vertices.len(),
                "a graph edge names two of its vertices"
            );
            assert!(
                vertices[edge.source].time < vertices[edge.target].time,
                "a graph edge follows the time axis"
            );
        }
        edges.sort_unstable_by_key(|edge| (edge.source, edge.target));
        for pair in edges.windows(2) {
            assert!(
                (pair[0].source, pair[0].target) != (pair[1].source, pair[1].target),
                "a graph edge appears once"
            );
        }
        Self { vertices, edges }
    }

    pub fn vertices(&self) -> &[Vertex] {
        &self.vertices
    }

    pub fn edges(&self) -> &[Edge] {
        &self.edges
    }

    pub fn len(&self) -> usize {
        self.vertices.len()
    }

    pub fn is_empty(&self) -> bool {
        self.vertices.is_empty()
    }

    pub fn times(&self) -> Vec<u32> {
        let mut times = self
            .vertices
            .iter()
            .map(|vertex| vertex.time)
            .collect::<Vec<_>>();
        times.dedup();
        times
    }

    pub fn divisions(&self) -> Vec<Division> {
        let mut children = std::collections::BTreeMap::<usize, Vec<usize>>::new();
        for edge in &self.edges {
            if edge.semantic == Semantic::Parent {
                children.entry(edge.source).or_default().push(edge.target);
            }
        }
        let mut divisions = Vec::new();
        for (parent, targets) in children {
            assert!(
                targets.len() <= 2,
                "a graph vertex leads to at most two parents"
            );
            if let [first, second] = targets[..] {
                divisions.push(Division {
                    parent,
                    children: [first, second],
                });
            }
        }
        divisions
    }

    pub fn of_tracks(set: &TrackSet) -> Self {
        let mut vertices = Vec::with_capacity(set.node_count());
        let mut first = Vec::with_capacity(set.len());
        let mut last = Vec::with_capacity(set.len());
        for track in set.tracks() {
            first.push(vertices.len());
            for node in track.nodes() {
                vertices.push(Vertex::new(node.time(), node.centroid()));
            }
            last.push(vertices.len() - 1);
        }
        let mut edges = Vec::with_capacity(set.node_count());
        for (index, track) in set.tracks().iter().enumerate() {
            for node in 1..track.nodes().len() {
                edges.push(Edge::new(
                    first[index] + node - 1,
                    first[index] + node,
                    Semantic::Track,
                ));
            }
        }
        let slots = set
            .tracks()
            .iter()
            .enumerate()
            .map(|(index, track)| (track.id(), index))
            .collect::<HashMap<_, _>>();
        for link in set.links() {
            let parent = last[slots[&link.parent()]];
            let child = first[slots[&link.child()]];
            let semantic = match link.kind() {
                Link::Gap | Link::Division | Link::Merge => Semantic::Parent,
            };
            edges.push(Edge::new(parent, child, semantic));
        }
        Self::new(vertices, edges)
    }
}
