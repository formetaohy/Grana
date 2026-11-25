use std::collections::HashMap;

use crate::NodeRef;
use crate::identity::{ObjectId, TrackId, TrackIdSource, TrackIds};
use crate::{Link, Track, TrackLink, TrackNode, TrackSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Continue,
    Gap,
    Division,
    Merge,
}

impl Kind {
    fn link(self) -> Option<Link> {
        match self {
            Self::Continue => None,
            Self::Gap => Some(Link::Gap),
            Self::Division => Some(Link::Division),
            Self::Merge => Some(Link::Merge),
        }
    }

    fn of(link: Link) -> Self {
        match link {
            Link::Gap => Self::Gap,
            Link::Division => Self::Division,
            Link::Merge => Self::Merge,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Detection {
    time: u32,
    id: ObjectId,
    centroid: [f32; 3],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Edge {
    source: usize,
    target: usize,
    kind: Kind,
}

struct Adjacency {
    outgoing: Vec<Vec<usize>>,
    incoming: Vec<Vec<usize>>,
}

impl Adjacency {
    fn of(detections: usize, edges: &[Edge]) -> Self {
        let mut outgoing = vec![Vec::new(); detections];
        let mut incoming = vec![Vec::new(); detections];
        for edge in edges {
            outgoing[edge.source].push(edge.target);
            incoming[edge.target].push(edge.source);
        }
        Self { outgoing, incoming }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Lineage {
    detections: Vec<Detection>,
    edges: Vec<Edge>,
    origins: HashMap<(u32, ObjectId), TrackId>,
}

impl Lineage {
    pub(crate) fn of(set: &TrackSet) -> Self {
        let mut detections = Vec::with_capacity(set.node_count());
        let mut edges = Vec::with_capacity(set.node_count());
        let mut first = Vec::with_capacity(set.len());
        let mut last = Vec::with_capacity(set.len());
        let mut origins = HashMap::with_capacity(set.len());
        for track in set.tracks() {
            let start = detections.len();
            for (index, node) in track.nodes().iter().enumerate() {
                if index == 0 {
                    origins.insert((node.time(), node.id()), track.id());
                }
                detections.push(Detection {
                    time: node.time(),
                    id: node.id(),
                    centroid: node.centroid(),
                });
                if index > 0 {
                    edges.push(Edge {
                        source: start + index - 1,
                        target: start + index,
                        kind: Kind::Continue,
                    });
                }
            }
            first.push(start);
            last.push(start + track.nodes().len() - 1);
        }
        let remap = remap_of(&detections);
        let detections = sorted_of(&detections, &remap);
        for edge in &mut edges {
            edge.source = remap[edge.source];
            edge.target = remap[edge.target];
        }
        for link in set.links() {
            edges.push(Edge {
                source: remap[last[set
                    .position(link.parent())
                    .expect("the parent track exists")]],
                target: remap[first[set.position(link.child()).expect("the child track exists")]],
                kind: Kind::of(link.kind()),
            });
        }
        Self {
            detections,
            edges,
            origins,
        }
        .normalized()
    }

    pub(crate) fn from_edges(
        nodes: &[(u32, ObjectId, [f32; 3])],
        edges: &[(NodeRef, NodeRef)],
    ) -> Result<Self, String> {
        let mut detections = nodes
            .iter()
            .map(|(time, id, centroid)| Detection {
                time: *time,
                id: *id,
                centroid: *centroid,
            })
            .collect::<Vec<_>>();
        let remap = remap_of(&detections);
        detections = sorted_of(&detections, &remap);
        if detections
            .windows(2)
            .any(|pair| (pair[0].time, pair[0].id) == (pair[1].time, pair[1].id))
        {
            return Err("a detection appears twice".to_string());
        }
        let mut held = Vec::new();
        for (source, target) in edges {
            let (Some(source), Some(target)) = (
                find(&detections, source.0, source.1),
                find(&detections, target.0, target.1),
            ) else {
                continue;
            };
            if source == target || detections[source].time >= detections[target].time {
                continue;
            }
            let kind = if detections[target].time == detections[source].time + 1 {
                Kind::Continue
            } else {
                Kind::Gap
            };
            held.push(Edge {
                source,
                target,
                kind,
            });
        }
        let mut lineage = Self {
            detections,
            edges: held,
            origins: HashMap::new(),
        };
        lineage.normalize();
        let adjacency = lineage.adjacency();
        for node in 0..lineage.detections.len() {
            if adjacency.incoming[node].len() > 2 {
                return Err(format!(
                    "the detection {} of the frame {} follows {} detections",
                    lineage.detections[node].id.raw(),
                    lineage.detections[node].time,
                    adjacency.incoming[node].len()
                ));
            }
            if adjacency.outgoing[node].len() > 2 {
                return Err(format!(
                    "the detection {} of the frame {} leads to {} detections",
                    lineage.detections[node].id.raw(),
                    lineage.detections[node].time,
                    adjacency.outgoing[node].len()
                ));
            }
        }
        lineage.check()?;
        Ok(lineage)
    }

    pub(crate) fn node_at(&self, time: u32, id: ObjectId) -> Option<usize> {
        find(&self.detections, time, id)
    }

    pub(crate) fn cut(&mut self, node: usize) -> bool {
        let before = self.edges.len();
        self.edges.retain(|edge| edge.source != node);
        if self.edges.len() == before {
            return false;
        }
        self.normalize();
        true
    }

    pub(crate) fn can_join(&self, source: usize, target: usize) -> bool {
        source != target
            && self.edges.iter().all(|edge| edge.source != source)
            && self.edges.iter().all(|edge| edge.target != target)
            && self.detections[target].time > self.detections[source].time
    }

    pub(crate) fn join(&mut self, source: usize, target: usize) {
        assert!(
            self.can_join(source, target),
            "a track joins a later detection that starts a lineage"
        );
        let kind = if self.detections[target].time == self.detections[source].time + 1 {
            Kind::Continue
        } else {
            Kind::Gap
        };
        self.edges.push(Edge {
            source,
            target,
            kind,
        });
        self.normalize();
    }

    pub(crate) fn can_divide(&self, parent: usize, first: usize, second: usize) -> bool {
        parent != first
            && parent != second
            && first != second
            && self.edges.iter().all(|edge| edge.source != parent)
            && self.edges.iter().all(|edge| edge.target != first)
            && self.edges.iter().all(|edge| edge.target != second)
            && self.detections[first].time > self.detections[parent].time
            && self.detections[second].time > self.detections[parent].time
    }

    pub(crate) fn divide(&mut self, parent: usize, first: usize, second: usize) {
        assert!(
            self.can_divide(parent, first, second),
            "a division links a track end to two lineage starts"
        );
        self.edges.push(Edge {
            source: parent,
            target: first,
            kind: Kind::Division,
        });
        self.edges.push(Edge {
            source: parent,
            target: second,
            kind: Kind::Division,
        });
        self.normalize();
    }

    pub(crate) fn can_merge(&self, first: usize, second: usize, child: usize) -> bool {
        let adjacency = self.adjacency();
        first != second
            && first != child
            && second != child
            && self.track_start(first) != self.track_start(second)
            && adjacency.outgoing[first].is_empty()
            && adjacency.outgoing[second].is_empty()
            && adjacency.incoming[child].len() < 2
            && adjacency.incoming[child]
                .iter()
                .all(|source| self.continues(*source) != Some(child))
            && adjacency.incoming[child]
                .iter()
                .all(|source| adjacency.outgoing[*source].len() == 1)
            && self.detections[child].time > self.detections[first].time
            && self.detections[child].time > self.detections[second].time
            && self
                .edges
                .iter()
                .all(|edge| (edge.source, edge.target) != (first, child))
            && self
                .edges
                .iter()
                .all(|edge| (edge.source, edge.target) != (second, child))
    }

    pub(crate) fn merge(&mut self, first: usize, second: usize, child: usize) {
        assert!(
            self.can_merge(first, second, child),
            "a merge joins two track ends to one lineage start"
        );
        self.edges.push(Edge {
            source: first,
            target: child,
            kind: Kind::Merge,
        });
        self.edges.push(Edge {
            source: second,
            target: child,
            kind: Kind::Merge,
        });
        self.normalize();
    }

    pub(crate) fn can_reparent(&self, child: usize, parent: usize) -> bool {
        parent != child
            && self.begins(child)
            && self.finishes(parent)
            && self
                .edges
                .iter()
                .filter(|edge| edge.source == parent)
                .count()
                < 2
            && self.detections[child].time > self.detections[parent].time
            && self
                .edges
                .iter()
                .all(|edge| (edge.source, edge.target) != (parent, child))
    }

    pub(crate) fn reparent(&mut self, child: usize, parent: usize) {
        assert!(
            self.can_reparent(child, parent),
            "a lineage start adopts an earlier track end"
        );
        self.edges.retain(|edge| edge.target != child);
        self.edges.push(Edge {
            source: parent,
            target: child,
            kind: Kind::Gap,
        });
        self.normalize();
    }

    pub(crate) fn can_detach(&self, child: usize) -> bool {
        self.begins(child) && self.edges.iter().any(|edge| edge.target == child)
    }

    pub(crate) fn detach(&mut self, child: usize) {
        self.edges.retain(|edge| edge.target != child);
        self.normalize();
    }

    pub(crate) fn without_objects(&mut self, time: u32, ids: &[ObjectId]) -> bool {
        if ids.is_empty() {
            return false;
        }
        let dropped = |detection: &Detection| {
            detection.time == time && ids.binary_search(&detection.id).is_ok()
        };
        let mut remap = vec![usize::MAX; self.detections.len()];
        let mut kept = Vec::with_capacity(self.detections.len());
        for (slot, detection) in self.detections.iter().enumerate() {
            if dropped(detection) {
                continue;
            }
            remap[slot] = kept.len();
            kept.push(*detection);
        }
        if kept.len() == self.detections.len() {
            return false;
        }
        self.detections = kept;
        self.edges = self
            .edges
            .iter()
            .filter_map(|edge| {
                let (source, target) = (remap[edge.source], remap[edge.target]);
                (source != usize::MAX && target != usize::MAX).then_some(Edge {
                    source,
                    target,
                    kind: edge.kind,
                })
            })
            .collect();
        self.normalize();
        true
    }

    pub(crate) fn into_set(
        mut self,
        frame_interval: f32,
        source: &TrackIdSource,
    ) -> Result<TrackSet, String> {
        self.normalize();
        self.check()?;
        let adjacency = self.adjacency();
        let count = self.detections.len();
        let mut chain_of = vec![usize::MAX; count];
        let mut chains: Vec<Vec<usize>> = Vec::new();
        for (start, incoming) in adjacency.incoming.iter().enumerate() {
            if incoming
                .iter()
                .any(|source| self.continues(*source) == Some(start))
            {
                continue;
            }
            let mut held = Vec::new();
            let mut current = start;
            while current != usize::MAX {
                chain_of[current] = chains.len();
                held.push(current);
                current = self.continues(current).unwrap_or(usize::MAX);
            }
            chains.push(held);
        }
        if chains.iter().map(Vec::len).sum::<usize>() != count {
            return Err("the detections of a lineage form a cycle".to_string());
        }
        let mut chosen = chains
            .iter()
            .map(|chain| {
                let first = self.detections[chain[0]];
                self.origins.get(&(first.time, first.id)).copied()
            })
            .collect::<Vec<_>>();
        source.observe(&TrackIds::new(
            chosen.iter().flatten().copied().collect::<Vec<_>>(),
        ));
        let missing = chosen.iter().filter(|id| id.is_none()).count();
        let allocated = source.allocate(missing);
        let mut allocated = allocated.as_slice().iter();
        for id in &mut chosen {
            if id.is_none() {
                *id = allocated.next().copied();
            }
        }
        let ids = chosen
            .into_iter()
            .map(|id| id.expect("a lineage chain starts a track"))
            .collect::<Vec<_>>();
        let mut tracks = Vec::with_capacity(chains.len());
        for (chain, id) in chains.iter().zip(&ids) {
            let nodes = chain
                .iter()
                .map(|slot| {
                    let detection = self.detections[*slot];
                    TrackNode::new(detection.time, detection.id, detection.centroid)
                })
                .collect::<Vec<_>>();
            tracks.push(Track::new(*id, nodes));
        }
        let mut links = Vec::new();
        for edge in &self.edges {
            let Some(kind) = edge.kind.link() else {
                continue;
            };
            links.push(TrackLink::new(
                ids[chain_of[edge.source]],
                ids[chain_of[edge.target]],
                kind,
            ));
        }
        TrackSet::assembled(frame_interval, tracks, links, source.clone())
    }

    fn continues(&self, node: usize) -> Option<usize> {
        self.edges
            .iter()
            .find(|edge| edge.source == node && edge.kind == Kind::Continue)
            .map(|edge| edge.target)
    }

    fn track_start(&self, node: usize) -> usize {
        let mut current = node;
        while let Some(edge) = self
            .edges
            .iter()
            .find(|edge| edge.target == current && edge.kind == Kind::Continue)
        {
            current = edge.source;
        }
        current
    }

    fn begins(&self, node: usize) -> bool {
        let adjacency = self.adjacency();
        adjacency.incoming[node]
            .iter()
            .all(|source| self.continues(*source) != Some(node))
    }

    fn finishes(&self, node: usize) -> bool {
        self.continues(node).is_none()
    }

    fn adjacency(&self) -> Adjacency {
        Adjacency::of(self.detections.len(), &self.edges)
    }

    fn normalize(&mut self) {
        self.edges
            .sort_unstable_by_key(|edge| (edge.source, edge.target));
        self.edges.dedup_by_key(|edge| (edge.source, edge.target));
        let adjacency = self.adjacency();
        for edge in &mut self.edges {
            if adjacency.outgoing[edge.source].len() == 2 {
                edge.kind = Kind::Division;
            } else if adjacency.incoming[edge.target].len() == 2 {
                edge.kind = Kind::Merge;
            } else if matches!(edge.kind, Kind::Division | Kind::Merge) {
                edge.kind = Kind::Gap;
            }
        }
    }

    fn normalized(mut self) -> Self {
        self.normalize();
        self
    }

    fn check(&self) -> Result<(), String> {
        let adjacency = self.adjacency();
        for node in 0..self.detections.len() {
            if adjacency.outgoing[node].len() == 2 && adjacency.incoming[node].len() == 2 {
                return Err("a detection divides into a merge".to_string());
            }
        }
        for edge in &self.edges {
            let consecutive =
                self.detections[edge.target].time == self.detections[edge.source].time + 1;
            let forwards = adjacency.outgoing[edge.source].len();
            let backwards = adjacency.incoming[edge.target].len();
            let valid = match edge.kind {
                Kind::Continue => consecutive && forwards == 1 && backwards == 1,
                Kind::Gap => forwards == 1 && backwards == 1,
                Kind::Division => forwards == 2,
                Kind::Merge => backwards == 2,
            };
            if !valid {
                return Err("a lineage link carries a kind its detections do not hold".to_string());
            }
        }
        Ok(())
    }
}

fn remap_of(detections: &[Detection]) -> Vec<usize> {
    let mut remap = vec![0usize; detections.len()];
    for (slot, source) in order(detections).into_iter().enumerate() {
        remap[source] = slot;
    }
    remap
}

fn sorted_of(detections: &[Detection], remap: &[usize]) -> Vec<Detection> {
    let mut sorted = vec![
        Detection {
            time: 0,
            id: ObjectId::FIRST,
            centroid: [0.0; 3],
        };
        detections.len()
    ];
    for (source, detection) in detections.iter().enumerate() {
        sorted[remap[source]] = *detection;
    }
    sorted
}

fn order(detections: &[Detection]) -> Vec<usize> {
    let mut order = (0..detections.len()).collect::<Vec<_>>();
    order.sort_unstable_by_key(|slot| (detections[*slot].time, detections[*slot].id));
    order
}

fn find(detections: &[Detection], time: u32, id: ObjectId) -> Option<usize> {
    detections
        .binary_search_by_key(&(time, id), |detection| (detection.time, detection.id))
        .ok()
}
