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
    object: usize,
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
}

impl Lineage {
    pub(crate) fn of(set: &TrackSet) -> Self {
        let mut detections = Vec::with_capacity(set.node_count());
        let mut edges = Vec::with_capacity(set.node_count());
        let mut first = Vec::with_capacity(set.len());
        let mut last = Vec::with_capacity(set.len());
        for track in set.tracks() {
            let start = detections.len();
            for (index, node) in track.nodes().iter().enumerate() {
                detections.push(Detection {
                    time: node.time(),
                    object: node.object(),
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
                source: remap[last[link.parent() as usize]],
                target: remap[first[link.child() as usize]],
                kind: Kind::of(link.kind()),
            });
        }
        Self { detections, edges }.normalized()
    }

    pub(crate) fn from_edges(
        nodes: &[(u32, usize, [f32; 3])],
        edges: &[([u32; 2], [u32; 2])],
    ) -> Result<Self, String> {
        let mut detections = nodes
            .iter()
            .map(|(time, object, centroid)| Detection {
                time: *time,
                object: *object,
                centroid: *centroid,
            })
            .collect::<Vec<_>>();
        let remap = remap_of(&detections);
        detections = sorted_of(&detections, &remap);
        if detections
            .windows(2)
            .any(|pair| (pair[0].time, pair[0].object) == (pair[1].time, pair[1].object))
        {
            return Err("a detection appears twice".to_string());
        }
        let mut held = Vec::new();
        for (source, target) in edges {
            let (Some(source), Some(target)) = (
                find(&detections, source[0], source[1] as usize),
                find(&detections, target[0], target[1] as usize),
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
        };
        lineage.normalize();
        let adjacency = lineage.adjacency();
        for node in 0..lineage.detections.len() {
            if adjacency.incoming[node].len() > 2 {
                return Err(format!(
                    "the detection {} of the frame {} follows {} detections",
                    lineage.detections[node].object,
                    lineage.detections[node].time,
                    adjacency.incoming[node].len()
                ));
            }
            if adjacency.outgoing[node].len() > 2 {
                return Err(format!(
                    "the detection {} of the frame {} leads to {} detections",
                    lineage.detections[node].object,
                    lineage.detections[node].time,
                    adjacency.outgoing[node].len()
                ));
            }
        }
        lineage.check()?;
        Ok(lineage)
    }

    pub(crate) fn node_at(&self, time: u32, object: usize) -> Option<usize> {
        find(&self.detections, time, object)
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

    pub(crate) fn without_objects(&mut self, time: u32, objects: &[usize]) -> bool {
        if objects.is_empty() {
            return false;
        }
        let mut kept = Vec::with_capacity(self.detections.len());
        let mut remap = vec![usize::MAX; self.detections.len()];
        let mut changed = false;
        for (slot, detection) in self.detections.iter().enumerate() {
            let removed = if detection.time == time {
                objects.partition_point(|object| *object < detection.object)
            } else {
                0
            };
            let dropped = detection.time == time && objects.get(removed) == Some(&detection.object);
            if dropped {
                changed = true;
                continue;
            }
            changed |= removed > 0;
            remap[slot] = kept.len();
            kept.push(Detection {
                object: detection.object - removed,
                ..*detection
            });
        }
        if !changed {
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

    pub(crate) fn shift(&mut self, offsets: &[usize]) {
        for detection in &mut self.detections {
            detection.object += offsets.get(detection.time as usize).copied().unwrap_or(0);
        }
        let remap = remap_of(&self.detections);
        self.detections = sorted_of(&self.detections, &remap);
        for edge in &mut self.edges {
            edge.source = remap[edge.source];
            edge.target = remap[edge.target];
        }
        self.normalize();
    }

    pub(crate) fn into_set(mut self, frame_interval: f32) -> Result<TrackSet, String> {
        self.normalize();
        self.check()?;
        let adjacency = self.adjacency();
        let count = self.detections.len();
        let mut chain_of = vec![usize::MAX; count];
        let mut chains: Vec<Vec<usize>> = Vec::new();
        for start in 0..count {
            if adjacency.incoming[start]
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
        let mut tracks = Vec::with_capacity(chains.len());
        for chain in &chains {
            let nodes = chain
                .iter()
                .map(|slot| {
                    let detection = self.detections[*slot];
                    TrackNode::new(detection.time, detection.object, detection.centroid)
                })
                .collect::<Vec<_>>();
            tracks.push(Track::new(nodes));
        }
        let mut links = Vec::new();
        for edge in &self.edges {
            let Some(kind) = edge.kind.link() else {
                continue;
            };
            links.push(TrackLink::new(
                chain_of[edge.source] as u32,
                chain_of[edge.target] as u32,
                kind,
            ));
        }
        TrackSet::try_with_links(frame_interval, tracks, links)
    }

    fn continues(&self, node: usize) -> Option<usize> {
        self.edges
            .iter()
            .find(|edge| edge.source == node && edge.kind == Kind::Continue)
            .map(|edge| edge.target)
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
            object: 0,
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
    order.sort_unstable_by_key(|slot| (detections[*slot].time, detections[*slot].object));
    order
}

fn find(detections: &[Detection], time: u32, object: usize) -> Option<usize> {
    detections
        .binary_search_by_key(&(time, object), |detection| {
            (detection.time, detection.object)
        })
        .ok()
}
