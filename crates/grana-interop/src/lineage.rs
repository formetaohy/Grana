use std::collections::HashMap;

use grana_object::{Spot, TrackSet};

use crate::error::Error;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Detection {
    id: u64,
    time: u32,
    position: [f32; 3],
    radius: f32,
    quality: f32,
}

impl Detection {
    pub fn new(id: u64, time: u32, position: [f32; 3], radius: f32, quality: f32) -> Self {
        assert!(
            position.iter().all(|value| value.is_finite()),
            "a detection lies at a finite position"
        );
        assert!(
            radius.is_finite() && radius > 0.0,
            "a detection spans a positive radius"
        );
        assert!(
            quality.is_finite(),
            "a detection carries a finite quality contrast"
        );
        Self {
            id,
            time,
            position,
            radius,
            quality,
        }
    }

    pub fn id(self) -> u64 {
        self.id
    }

    pub fn time(self) -> u32 {
        self.time
    }

    pub fn position(self) -> [f32; 3] {
        self.position
    }

    pub fn radius(self) -> f32 {
        self.radius
    }

    pub fn quality(self) -> f32 {
        self.quality
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Edge {
    source: u64,
    target: u64,
}

impl Edge {
    pub fn new(source: u64, target: u64) -> Self {
        Self { source, target }
    }

    pub fn source(self) -> u64 {
        self.source
    }

    pub fn target(self) -> u64 {
        self.target
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Lineage {
    detections: Vec<Detection>,
    edges: Vec<Edge>,
}

impl Lineage {
    pub fn new(mut detections: Vec<Detection>, mut edges: Vec<Edge>) -> Result<Self, Error> {
        detections.sort_by_key(|detection| (detection.time, detection.id));
        let mut slots: HashMap<u64, usize> = HashMap::with_capacity(detections.len());
        for (slot, detection) in detections.iter().enumerate() {
            if slots.insert(detection.id, slot).is_some() {
                return Err(Error::Format {
                    detail: format!("the detection {} appears twice", detection.id),
                });
            }
        }
        edges.sort_by_key(|edge| (edge.source, edge.target));
        edges.dedup();
        let mut parents: HashMap<u64, Vec<u64>> = HashMap::with_capacity(edges.len());
        for edge in &edges {
            let source = *slots.get(&edge.source).ok_or_else(|| Error::Format {
                detail: format!("the link starts at the unknown spot {}", edge.source),
            })?;
            let target = *slots.get(&edge.target).ok_or_else(|| Error::Format {
                detail: format!("the link ends at the unknown spot {}", edge.target),
            })?;
            if edge.source == edge.target {
                return Err(Error::Format {
                    detail: format!("the spot {} links to itself", edge.source),
                });
            }
            if detections[source].time >= detections[target].time {
                return Err(Error::Format {
                    detail: format!(
                        "the link {} to {} does not advance in time",
                        edge.source, edge.target
                    ),
                });
            }
            let previous = parents.entry(edge.target).or_default();
            if previous.len() == 2 {
                return Err(Error::Format {
                    detail: format!("the spot {} follows more than two spots", edge.target),
                });
            }
            previous.push(edge.source);
        }
        Ok(Self { detections, edges })
    }

    pub fn detections(&self) -> &[Detection] {
        &self.detections
    }

    pub fn edges(&self) -> &[Edge] {
        &self.edges
    }

    pub fn len(&self) -> usize {
        self.detections.len()
    }

    pub fn is_empty(&self) -> bool {
        self.detections.is_empty()
    }

    pub fn frames(&self) -> Vec<u32> {
        let mut times = self
            .detections
            .iter()
            .map(|detection| detection.time)
            .collect::<Vec<_>>();
        times.dedup();
        times
    }

    pub fn spots(&self) -> Vec<(u32, Vec<Spot>)> {
        let mut frames: Vec<(u32, Vec<Spot>)> = Vec::new();
        for detection in &self.detections {
            let spot = Spot::new(detection.position, detection.radius, detection.quality);
            match frames.last_mut() {
                Some((time, spots)) if *time == detection.time => spots.push(spot),
                _ => frames.push((detection.time, vec![spot])),
            }
        }
        frames
    }

    pub fn tracks(&self, frame_interval: f32) -> Result<TrackSet, Error> {
        let mut keys: HashMap<u64, [u32; 2]> = HashMap::with_capacity(self.detections.len());
        let mut order: HashMap<u32, usize> = HashMap::new();
        let mut nodes = Vec::with_capacity(self.detections.len());
        for detection in &self.detections {
            let ordinal = order.entry(detection.time).or_insert(0);
            keys.insert(detection.id, [detection.time, *ordinal as u32]);
            nodes.push((detection.time, *ordinal, detection.position));
            *ordinal += 1;
        }
        let edges = self
            .edges
            .iter()
            .map(|edge| (keys[&edge.source], keys[&edge.target]))
            .collect::<Vec<_>>();
        TrackSet::linked(frame_interval, &nodes, &edges)
            .map_err(|detail| Error::Format { detail })?
            .ok_or_else(|| Error::Format {
                detail: "the lineage holds no spot to track".to_string(),
            })
    }
}
