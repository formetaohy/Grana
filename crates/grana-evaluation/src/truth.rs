use std::collections::{BTreeMap, BTreeSet};

use crate::graph::{Edge, Graph, Semantic, Vertex};
use crate::marker::{Marker, TrackRow};

#[derive(Clone, Debug, PartialEq)]
pub struct Truth {
    frames: Vec<Vec<Marker>>,
    rows: Vec<TrackRow>,
    first: Vec<usize>,
    radii: Vec<[f32; 3]>,
    graph: Graph,
}

impl Truth {
    pub fn new(frames: Vec<Vec<Marker>>, rows: Vec<TrackRow>) -> Result<Self, String> {
        if frames.is_empty() {
            return Err("a reference series holds at least one frame".to_string());
        }
        if frames.iter().all(Vec::is_empty) {
            return Err("a reference series marks at least one object".to_string());
        }
        let labels = rows.iter().map(|row| row.label()).collect::<BTreeSet<_>>();
        if labels.len() != rows.len() {
            return Err("every reference track carries one row".to_string());
        }
        let mut counts = BTreeMap::<u32, usize>::new();
        for (time, markers) in frames.iter().enumerate() {
            let mut seen = BTreeSet::new();
            for marker in markers {
                if marker.time() as usize != time {
                    return Err(format!(
                        "the marker of the track {} names the frame {} in the frame {time}",
                        marker.label(),
                        marker.time()
                    ));
                }
                if !seen.insert(marker.label()) {
                    return Err(format!(
                        "the track {} marks the frame {time} twice",
                        marker.label()
                    ));
                }
            }
            for label in seen {
                *counts.entry(label).or_default() += 1;
            }
        }
        for label in counts.keys() {
            if !labels.contains(label) {
                return Err(format!("the track {label} holds no reference row"));
            }
        }
        let mut rows_of = BTreeMap::<u32, TrackRow>::new();
        for row in &rows {
            rows_of.insert(row.label(), *row);
        }
        for row in &rows {
            if row.end() as usize >= frames.len() {
                return Err(format!(
                    "the track {} spans frames beyond the series",
                    row.label()
                ));
            }
            let counted = counts.get(&row.label()).copied().unwrap_or(0);
            if counted != (row.end() - row.begin() + 1) as usize {
                return Err(format!(
                    "the track {} marks {} frames of its span",
                    row.label(),
                    counted
                ));
            }
            if row.parent() == 0 {
                continue;
            }
            let Some(parent) = rows_of.get(&row.parent()) else {
                return Err(format!(
                    "the track {} follows the missing track {}",
                    row.label(),
                    row.parent()
                ));
            };
            if parent.end() >= row.begin() {
                return Err(format!(
                    "the track {} ends after its daughter {} begins",
                    parent.label(),
                    row.label()
                ));
            }
        }
        let mut children = BTreeMap::<u32, usize>::new();
        for row in &rows {
            if row.parent() == 0 {
                continue;
            }
            let held = children.entry(row.parent()).or_default();
            *held += 1;
            if *held > 2 {
                return Err(format!(
                    "the track {} leads to more than two daughters",
                    row.parent()
                ));
            }
        }
        let mut vertices = Vec::new();
        let mut radii = Vec::new();
        let mut first = Vec::with_capacity(frames.len());
        let mut start = BTreeMap::<u32, usize>::new();
        let mut last = BTreeMap::<u32, usize>::new();
        let mut edges = Vec::new();
        for markers in &frames {
            first.push(vertices.len());
            for marker in markers {
                let index = vertices.len();
                vertices.push(Vertex::new(marker.time(), marker.position()));
                radii.push(marker.radii());
                start.entry(marker.label()).or_insert(index);
                if let Some(previous) = last.insert(marker.label(), index) {
                    edges.push(Edge::new(previous, index, Semantic::Track));
                }
            }
        }
        for row in &rows {
            if row.parent() == 0 {
                continue;
            }
            let (Some(parent), Some(child)) = (last.get(&row.parent()), start.get(&row.label()))
            else {
                return Err(format!(
                    "the track {} links two tracks that mark no frame",
                    row.label()
                ));
            };
            edges.push(Edge::new(*parent, *child, Semantic::Parent));
        }
        Ok(Self {
            graph: Graph::new(vertices, edges),
            frames,
            rows,
            first,
            radii,
        })
    }

    pub fn frames(&self) -> &[Vec<Marker>] {
        &self.frames
    }

    pub fn rows(&self) -> &[TrackRow] {
        &self.rows
    }

    pub fn graph(&self) -> &Graph {
        &self.graph
    }

    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    pub fn first_vertex(&self, time: usize) -> usize {
        self.first[time]
    }

    pub fn radii(&self, vertex: usize) -> [f32; 3] {
        self.radii[vertex]
    }
}
