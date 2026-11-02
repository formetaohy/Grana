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
    outgoing: Vec<[Option<usize>; 2]>,
}

impl TrackGraph {
    pub fn from_set(set: &TrackSet) -> Self {
        let mut graph = Self {
            detections: Vec::new(),
            incoming: Vec::new(),
            outgoing: Vec::new(),
        };
        let mut spans = Vec::with_capacity(set.len());
        for track in set.tracks() {
            let first = graph.detections.len();
            for node in track.nodes() {
                let index = graph.push(*node);
                if index > first {
                    graph.link(index - 1, index);
                }
            }
            let last = graph.detections.len() - 1;
            spans.push((first, last));
        }
        for (index, track) in set.tracks().iter().enumerate() {
            if let Some(parent) = track.parent() {
                let parent = parent as usize;
                assert!(parent < index, "a track follows its parent");
                graph.link(spans[parent].1, spans[index].0);
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

    pub fn cut(&mut self, node: usize) -> bool {
        let mut cut = false;
        for slot in &mut self.outgoing[node] {
            if let Some(target) = slot.take() {
                self.incoming[target] = None;
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
        self.outgoing[source][0] = Some(target);
        self.incoming[target] = Some(source);
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
                match self.outgoing[current] {
                    [Some(next), None] => current = next,
                    _ => break,
                }
            }
            let parent = self.incoming[start].map(|previous| {
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
        self.outgoing.push([None, None]);
        index
    }

    fn link(&mut self, source: usize, target: usize) {
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
            .iter_mut()
            .find(|slot| slot.is_none())
            .expect("a detection precedes at most two detections");
        *slot = Some(target);
        self.incoming[target] = Some(source);
    }

    fn starts(&self, node: usize) -> bool {
        match self.incoming[node] {
            None => true,
            Some(previous) => self.outgoing[previous][1].is_some(),
        }
    }

    fn check(&self) {
        for node in 0..self.len() {
            for target in self.outgoing[node].iter().flatten() {
                assert_eq!(
                    self.incoming[*target],
                    Some(node),
                    "every link continues its detection"
                );
            }
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
