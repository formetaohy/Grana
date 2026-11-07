use std::collections::HashMap;

use grana_object::{Spot, Track, TrackNode, TrackSet};

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrackPath {
    parent: Option<usize>,
    nodes: Vec<u64>,
}

impl TrackPath {
    pub fn parent(&self) -> Option<usize> {
        self.parent
    }

    pub fn nodes(&self) -> &[u64] {
        &self.nodes
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
        let mut parents: HashMap<u64, u64> = HashMap::with_capacity(edges.len());
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
            if let Some(previous) = parents.insert(edge.target, edge.source) {
                return Err(Error::Format {
                    detail: format!(
                        "the spot {} follows both {previous} and {}",
                        edge.target, edge.source
                    ),
                });
            }
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
        let n = self.detections.len();
        let mut slots: HashMap<u64, usize> = HashMap::with_capacity(n);
        for (slot, detection) in self.detections.iter().enumerate() {
            slots.insert(detection.id, slot);
        }
        let mut objects = vec![0usize; n];
        let mut order: HashMap<u32, usize> = HashMap::new();
        for (slot, detection) in self.detections.iter().enumerate() {
            let ordinal = order.entry(detection.time).or_insert(0);
            objects[slot] = *ordinal;
            *ordinal += 1;
        }
        let mut successors = vec![Vec::new(); n];
        let mut predecessors = vec![None; n];
        for edge in &self.edges {
            let source = slots[&edge.source];
            let target = slots[&edge.target];
            successors[source].push(target);
            predecessors[target] = Some(source);
        }
        for (slot, next) in successors.iter().enumerate() {
            if next.len() > 2 {
                return Err(Error::Format {
                    detail: format!(
                        "the spot {} links to {} daughters",
                        self.detections[slot].id,
                        next.len()
                    ),
                });
            }
        }
        let mut tracks: Vec<Track> = Vec::new();
        let mut visited = vec![false; n];
        let mut pending = predecessors
            .iter()
            .enumerate()
            .filter(|(_, previous)| previous.is_none())
            .map(|(slot, _)| (slot, None))
            .collect::<Vec<_>>();
        pending.reverse();
        while let Some((start, parent)) = pending.pop() {
            let mut nodes = Vec::new();
            let mut current = start;
            let mut daughters = Vec::new();
            loop {
                if visited[current] {
                    return Err(Error::Format {
                        detail: format!(
                            "the spot {} belongs to two tracks",
                            self.detections[current].id
                        ),
                    });
                }
                visited[current] = true;
                let detection = self.detections[current];
                nodes.push(TrackNode::new(
                    detection.time,
                    objects[current],
                    detection.position,
                ));
                match successors[current].as_slice() {
                    [] => break,
                    [only] => current = *only,
                    [first, second] => {
                        daughters = vec![*first, *second];
                        break;
                    }
                    _ => unreachable!("a spot links to at most two daughters"),
                }
            }
            let index = tracks.len() as u32;
            tracks.push(Track::new(parent, nodes));
            daughters.reverse();
            for daughter in daughters {
                pending.push((daughter, Some(index)));
            }
        }
        if let Some(slot) = visited.iter().position(|visited| !visited) {
            return Err(Error::Format {
                detail: format!(
                    "the spot {} closes a cycle instead of following a track",
                    self.detections[slot].id
                ),
            });
        }
        TrackSet::try_new(frame_interval, tracks).map_err(|detail| Error::Format { detail })
    }

    pub fn track_paths(&self, frame_interval: f32) -> Result<Vec<TrackPath>, Error> {
        let set = self.tracks(frame_interval)?;
        let mut ids = HashMap::with_capacity(self.detections.len());
        let mut order: HashMap<u32, usize> = HashMap::new();
        for detection in &self.detections {
            let ordinal = order.entry(detection.time).or_insert(0);
            ids.insert((detection.time, *ordinal), detection.id);
            *ordinal += 1;
        }
        Ok(set
            .tracks()
            .iter()
            .map(|track| TrackPath {
                parent: track.parent().map(|parent| parent as usize),
                nodes: track
                    .nodes()
                    .iter()
                    .map(|node| ids[&(node.time(), node.object())])
                    .collect(),
            })
            .collect())
    }
}
