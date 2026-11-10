use crate::{Track, TrackNode, TrackSet};

#[derive(Clone, Copy, Debug, PartialEq)]
struct Detection {
    time: u32,
    object: usize,
    centroid: [f32; 3],
}

#[derive(Clone, Debug)]
pub(crate) struct TrackGraph {
    detections: Vec<Detection>,
    incoming: Vec<Option<usize>>,
    parented: Vec<bool>,
    outgoing: Vec<[Option<usize>; 2]>,
    boundary: Vec<[bool; 2]>,
}

impl TrackGraph {
    pub fn from_set(set: &TrackSet) -> Self {
        let mut graph = Self {
            detections: Vec::new(),
            incoming: Vec::new(),
            parented: Vec::new(),
            outgoing: Vec::new(),
            boundary: Vec::new(),
        };
        let mut spans = Vec::with_capacity(set.len());
        for track in set.tracks() {
            let first = graph.detections.len();
            for node in track.nodes() {
                let index = graph.push(*node);
                if index > first {
                    graph.link(index - 1, index, false);
                }
            }
            let last = graph.detections.len() - 1;
            spans.push((first, last));
        }
        for (index, track) in set.tracks().iter().enumerate() {
            if let Some(parent) = track.parent() {
                let parent = parent as usize;
                assert!(parent < index, "a track follows its parent");
                graph.link(spans[parent].1, spans[index].0, true);
            }
        }
        graph.check();
        graph
    }

    pub fn len(&self) -> usize {
        self.detections.len()
    }

    pub fn node_at(&self, time: u32, object: usize) -> Option<usize> {
        self.detections
            .iter()
            .position(|detection| detection.time == time && detection.object == object)
    }

    pub fn starts(&self, node: usize) -> bool {
        self.incoming[node].is_none() || self.parented[node]
    }

    pub fn ends(&self, node: usize) -> bool {
        self.outgoing[node]
            .iter()
            .zip(&self.boundary[node])
            .all(|(target, boundary)| target.is_none() || *boundary)
    }

    pub fn children(&self, node: usize) -> usize {
        (0..2)
            .filter(|slot| self.outgoing[node][*slot].is_some() && self.boundary[node][*slot])
            .count()
    }

    pub fn cut(&mut self, node: usize) -> bool {
        let mut cut = false;
        for slot in 0..2 {
            if let Some(target) = self.outgoing[node][slot].take() {
                self.incoming[target] = None;
                self.parented[target] = false;
                self.boundary[node][slot] = false;
                cut = true;
            }
        }
        cut
    }

    pub fn can_join(&self, source: usize, target: usize) -> bool {
        source != target
            && self.outgoing[source].iter().all(Option::is_none)
            && self.incoming[target].is_none()
            && self.detections[target].time > self.detections[source].time
    }

    pub fn join(&mut self, source: usize, target: usize) {
        assert!(
            self.can_join(source, target),
            "a track joins a later detection that starts a lineage"
        );
        self.link(source, target, false);
    }

    pub fn can_divide(&self, parent: usize, first: usize, second: usize) -> bool {
        parent != first
            && parent != second
            && first != second
            && self.ends(parent)
            && self.children(parent) == 0
            && self.starts(first)
            && self.starts(second)
            && self.detections[first].time > self.detections[parent].time
            && self.detections[second].time > self.detections[parent].time
    }

    pub fn divide(&mut self, parent: usize, first: usize, second: usize) {
        assert!(
            self.can_divide(parent, first, second),
            "a division links a track end to two lineage starts"
        );
        self.link(parent, first, true);
        self.link(parent, second, true);
    }

    pub fn can_reparent(&self, child: usize, parent: usize) -> bool {
        parent != child
            && self.starts(child)
            && self.ends(parent)
            && self.children(parent) < 2
            && self.incoming[child] != Some(parent)
            && self.detections[child].time > self.detections[parent].time
    }

    pub fn reparent(&mut self, child: usize, parent: usize) {
        assert!(
            self.can_reparent(child, parent),
            "a lineage start adopts an earlier track end"
        );
        self.detach(child);
        self.link(parent, child, true);
    }

    pub fn can_detach(&self, child: usize) -> bool {
        self.starts(child) && self.parented[child]
    }

    pub fn detach(&mut self, child: usize) {
        let Some(parent) = self.incoming[child] else {
            return;
        };
        for slot in 0..2 {
            if self.outgoing[parent][slot] == Some(child) {
                self.outgoing[parent][slot] = None;
                self.boundary[parent][slot] = false;
            }
        }
        self.incoming[child] = None;
        self.parented[child] = false;
    }

    pub fn into_set(self, frame_interval: f32) -> TrackSet {
        let mut starts = (0..self.len())
            .filter(|node| self.starts(*node))
            .collect::<Vec<_>>();
        starts.sort_unstable_by_key(|node| (self.detections[*node].time, *node));
        let mut owner = vec![None; self.len()];
        let mut tracks = Vec::with_capacity(starts.len());
        for start in starts {
            let mut chain = Vec::new();
            let mut current = start;
            loop {
                assert!(
                    owner[current].is_none(),
                    "every detection belongs to one track"
                );
                owner[current] = Some(tracks.len());
                let detection = self.detections[current];
                chain.push(TrackNode::new(
                    detection.time,
                    detection.object,
                    detection.centroid,
                ));
                let next = (0..2).find(|slot| {
                    self.outgoing[current][*slot].is_some() && !self.boundary[current][*slot]
                });
                match next {
                    Some(slot) => {
                        current = self.outgoing[current][slot].expect("the link holds a target")
                    }
                    None => break,
                }
            }
            let parent = self.parented[start]
                .then(|| self.incoming[start])
                .flatten()
                .map(|previous| {
                    owner[previous].expect("a parent track precedes its daughters") as u32
                });
            tracks.push(Track::new(parent, chain));
        }
        TrackSet::new(frame_interval, tracks)
    }

    fn push(&mut self, node: TrackNode) -> usize {
        let index = self.detections.len();
        self.detections.push(Detection {
            time: node.time(),
            object: node.object(),
            centroid: node.centroid(),
        });
        self.incoming.push(None);
        self.parented.push(false);
        self.outgoing.push([None, None]);
        self.boundary.push([false, false]);
        index
    }

    fn link(&mut self, source: usize, target: usize, boundary: bool) {
        assert!(source != target, "a link joins two detections");
        assert!(
            self.detections[target].time > self.detections[source].time,
            "a link follows the time axis"
        );
        assert!(
            self.incoming[target].is_none(),
            "a detection continues one detection"
        );
        let slot = self.outgoing[source]
            .iter()
            .position(Option::is_none)
            .expect("a detection precedes at most two detections");
        self.outgoing[source][slot] = Some(target);
        self.boundary[source][slot] = boundary;
        self.incoming[target] = Some(source);
        self.parented[target] = boundary;
    }

    fn check(&self) {
        for node in 0..self.len() {
            let mut continuing = 0;
            for slot in 0..2 {
                if let Some(target) = self.outgoing[node][slot] {
                    assert_eq!(
                        self.incoming[target],
                        Some(node),
                        "every link continues its detection"
                    );
                    assert_eq!(
                        self.parented[target], self.boundary[node][slot],
                        "a parent link names a lineage start"
                    );
                    if !self.boundary[node][slot] {
                        continuing += 1;
                    }
                }
            }
            assert!(
                continuing <= 1,
                "a detection continues at most one detection"
            );
            if let Some(previous) = self.incoming[node] {
                assert!(
                    self.outgoing[previous].contains(&Some(node)),
                    "every detection follows its link"
                );
                assert!(
                    self.detections[previous].time < self.detections[node].time,
                    "a link follows the time axis"
                );
            }
        }
        let mut keys = self
            .detections
            .iter()
            .map(|detection| (detection.time, detection.object))
            .collect::<Vec<_>>();
        keys.sort_unstable();
        assert!(
            keys.windows(2).all(|pair| pair[0] != pair[1]),
            "every detection belongs to one track"
        );
    }
}
