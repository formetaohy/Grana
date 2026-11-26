use std::collections::{HashMap, HashSet};

use crate::TrackLayout;
use crate::identity::{ObjectId, TrackId, TrackIdSource, TrackIds};
use crate::lineage::Lineage;

pub type NodeRef = (u32, ObjectId);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrackDetection {
    id: ObjectId,
    centroid: [f32; 3],
    radii: [f32; 3],
}

impl TrackDetection {
    pub fn new(id: ObjectId, centroid: [f32; 3], radii: [f32; 3]) -> Self {
        assert!(
            centroid.iter().all(|value| value.is_finite()),
            "a tracked detection lies at a finite centroid"
        );
        assert!(
            radii.iter().all(|value| value.is_finite() && *value > 0.0),
            "a tracked detection spans a positive radius on every axis"
        );
        Self {
            id,
            centroid,
            radii,
        }
    }

    pub fn id(self) -> ObjectId {
        self.id
    }

    pub fn centroid(self) -> [f32; 3] {
        self.centroid
    }

    pub fn radii(self) -> [f32; 3] {
        self.radii
    }

    pub fn radius(self) -> f32 {
        grana_geometry::Ellipsoid::new(self.centroid, self.radii).equivalent_radius()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrackNode {
    time: u32,
    id: ObjectId,
    centroid: [f32; 3],
}

impl TrackNode {
    pub fn new(time: u32, id: ObjectId, centroid: [f32; 3]) -> Self {
        assert!(
            centroid.iter().all(|value| value.is_finite()),
            "a track node lies at a finite centroid"
        );
        Self { time, id, centroid }
    }

    pub fn time(self) -> u32 {
        self.time
    }

    pub fn id(self) -> ObjectId {
        self.id
    }

    pub fn centroid(self) -> [f32; 3] {
        self.centroid
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Link {
    Gap,
    Division,
    Merge,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SolvedConfidence(f64);

impl SolvedConfidence {
    pub fn new(value: f64) -> Option<Self> {
        (value.is_finite() && value > 0.0 && value <= 1.0).then_some(Self(value))
    }

    pub fn value(self) -> f64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LinkConfidence {
    Solved(SolvedConfidence),
    Manual,
}

impl LinkConfidence {
    pub fn value(self) -> Option<f64> {
        match self {
            Self::Solved(confidence) => Some(confidence.value()),
            Self::Manual => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrackLink {
    parent: TrackId,
    child: TrackId,
    kind: Link,
    confidence: LinkConfidence,
}

impl TrackLink {
    pub fn new(parent: TrackId, child: TrackId, kind: Link) -> Self {
        assert!(parent != child, "a track link joins two tracks");
        Self {
            parent,
            child,
            kind,
            confidence: LinkConfidence::Manual,
        }
    }

    pub fn solved(
        parent: TrackId,
        child: TrackId,
        kind: Link,
        confidence: SolvedConfidence,
    ) -> Self {
        let mut link = Self::new(parent, child, kind);
        link.confidence = LinkConfidence::Solved(confidence);
        link
    }

    pub fn parent(self) -> TrackId {
        self.parent
    }

    pub fn child(self) -> TrackId {
        self.child
    }

    pub fn kind(self) -> Link {
        self.kind
    }

    pub fn confidence(self) -> LinkConfidence {
        self.confidence
    }

    fn reidentified(self, parent: TrackId, child: TrackId) -> Self {
        Self {
            parent,
            child,
            ..self
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrackStatistics {
    frames: u32,
    duration: f64,
    length: f64,
    displacement: f64,
    speed: Option<f64>,
    straightness: Option<f64>,
}

impl TrackStatistics {
    pub fn frames(self) -> u32 {
        self.frames
    }

    pub fn duration(self) -> f64 {
        self.duration
    }

    pub fn length(self) -> f64 {
        self.length
    }

    pub fn displacement(self) -> f64 {
        self.displacement
    }

    pub fn speed(self) -> Option<f64> {
        self.speed
    }

    pub fn straightness(self) -> Option<f64> {
        self.straightness
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Track {
    id: TrackId,
    nodes: Vec<TrackNode>,
}

impl Track {
    pub fn new(id: TrackId, nodes: Vec<TrackNode>) -> Self {
        assert!(!nodes.is_empty(), "a track holds at least one node");
        for pair in nodes.windows(2) {
            assert!(
                pair[1].time == pair[0].time + 1,
                "track nodes follow consecutive frames"
            );
        }
        Self { id, nodes }
    }

    pub fn id(&self) -> TrackId {
        self.id
    }

    pub fn nodes(&self) -> &[TrackNode] {
        &self.nodes
    }

    pub fn start(&self) -> u32 {
        self.nodes.first().expect("a track holds nodes").time
    }

    pub fn end(&self) -> u32 {
        self.nodes.last().expect("a track holds nodes").time
    }

    pub fn statistics(&self, frame_interval: f32) -> TrackStatistics {
        assert!(
            frame_interval.is_finite() && frame_interval > 0.0,
            "the frame interval is a positive duration"
        );
        let first = self.nodes.first().expect("a track holds nodes");
        let last = self.nodes.last().expect("a track holds nodes");
        let frames = self.nodes.len() as u32;
        let duration = f64::from(frames - 1) * f64::from(frame_interval);
        let length = self
            .nodes
            .windows(2)
            .map(|pair| distance(pair[0].centroid, pair[1].centroid))
            .sum::<f64>();
        let displacement = distance(first.centroid, last.centroid);
        let speed = (duration > 0.0).then(|| length / duration);
        let straightness = (length > 0.0).then(|| (displacement / length).clamp(0.0, 1.0));
        TrackStatistics {
            frames,
            duration,
            length,
            displacement,
            speed,
            straightness,
        }
    }

    fn reidentified(&self, id: TrackId) -> Self {
        Self::new(id, self.nodes.clone())
    }
}

#[derive(Clone, Debug)]
pub struct TrackSet {
    frame_interval: f32,
    tracks: Vec<Track>,
    links: Vec<TrackLink>,
    by_child: Vec<u32>,
    detections: Vec<(u32, ObjectId, TrackId)>,
    divisions: usize,
    merges: usize,
    layout: TrackLayout,
    source: TrackIdSource,
}

impl PartialEq for TrackSet {
    fn eq(&self, other: &Self) -> bool {
        self.frame_interval == other.frame_interval
            && self.tracks == other.tracks
            && self.links == other.links
    }
}

impl TrackSet {
    pub fn new(frame_interval: f32, tracks: Vec<Track>) -> Self {
        Self::with_links(frame_interval, tracks, Vec::new())
    }

    pub fn with_links(frame_interval: f32, tracks: Vec<Track>, links: Vec<TrackLink>) -> Self {
        match Self::try_with_links(frame_interval, tracks, links) {
            Ok(set) => set,
            Err(detail) => panic!("{detail}"),
        }
    }

    pub fn try_new(frame_interval: f32, tracks: Vec<Track>) -> Result<Self, String> {
        Self::try_with_links(frame_interval, tracks, Vec::new())
    }

    pub fn try_with_links(
        frame_interval: f32,
        tracks: Vec<Track>,
        links: Vec<TrackLink>,
    ) -> Result<Self, String> {
        Self::assembled(frame_interval, tracks, links, TrackIdSource::new())
    }

    pub(crate) fn assembled(
        frame_interval: f32,
        mut tracks: Vec<Track>,
        mut links: Vec<TrackLink>,
        source: TrackIdSource,
    ) -> Result<Self, String> {
        if !(frame_interval.is_finite() && frame_interval > 0.0) {
            return Err("the frame interval is a positive duration".to_string());
        }
        tracks.sort_unstable_by_key(Track::id);
        if tracks.windows(2).any(|pair| pair[0].id() == pair[1].id()) {
            return Err("a track id appears twice".to_string());
        }
        if let Some(ids) = TrackIds::checked(tracks.iter().map(Track::id).collect()) {
            source.observe(&ids);
        }
        let mut detections = Vec::new();
        for track in &tracks {
            if track.nodes().is_empty() {
                return Err("a track holds at least one node".to_string());
            }
            if track
                .nodes()
                .windows(2)
                .any(|pair| pair[1].time() != pair[0].time() + 1)
            {
                return Err("track nodes follow consecutive frames".to_string());
            }
            for node in track.nodes() {
                detections.push((node.time(), node.id(), track.id()));
            }
        }
        detections.sort_unstable();
        if detections
            .windows(2)
            .any(|pair| (pair[0].0, pair[0].1) == (pair[1].0, pair[1].1))
        {
            return Err("every detection belongs to one track".to_string());
        }
        links.sort_unstable_by_key(|link| (link.parent().raw(), link.child().raw()));
        if links
            .windows(2)
            .any(|pair| (pair[0].parent(), pair[0].child()) == (pair[1].parent(), pair[1].child()))
        {
            return Err("a track link appears twice".to_string());
        }
        let slot = tracks
            .iter()
            .enumerate()
            .map(|(index, track)| (track.id(), index))
            .collect::<HashMap<_, _>>();
        let mut incoming = vec![0u8; tracks.len()];
        let mut outgoing = vec![0u8; tracks.len()];
        for link in &links {
            let parent = *slot
                .get(&link.parent())
                .ok_or_else(|| "a track link names a missing track".to_string())?;
            let child = *slot
                .get(&link.child())
                .ok_or_else(|| "a track link names a missing track".to_string())?;
            if tracks[parent].end() >= tracks[child].start() {
                return Err("a child track starts after its parent ends".to_string());
            }
            if incoming[child] == 2 || outgoing[parent] == 2 {
                return Err("a track follows at most two tracks and leads to two".to_string());
            }
            incoming[child] += 1;
            outgoing[parent] += 1;
        }
        for link in &links {
            let parent = slot[&link.parent()];
            let child = slot[&link.child()];
            if outgoing[parent] == 2 && incoming[child] == 2 {
                return Err("a track divides into a merge".to_string());
            }
            let kind = if outgoing[parent] == 2 {
                Link::Division
            } else if incoming[child] == 2 {
                Link::Merge
            } else {
                Link::Gap
            };
            if link.kind != kind {
                return Err("a track link carries the wrong kind".to_string());
            }
        }
        let divisions = outgoing.iter().filter(|count| **count == 2).count();
        let merges = incoming.iter().filter(|count| **count == 2).count();
        let mut by_child = (0..links.len() as u32).collect::<Vec<_>>();
        by_child.sort_unstable_by_key(|index| (links[*index as usize].child().raw(), *index));
        let mut set = Self {
            frame_interval,
            tracks,
            links,
            by_child,
            detections,
            divisions,
            merges,
            layout: TrackLayout::default(),
            source,
        };
        set.layout = TrackLayout::of(&set);
        Ok(set)
    }

    pub fn linked(
        frame_interval: f32,
        nodes: &[(u32, ObjectId, [f32; 3])],
        edges: &[(NodeRef, NodeRef)],
    ) -> Result<Option<Self>, String> {
        if nodes.is_empty() {
            return Ok(None);
        }
        Lineage::from_edges(nodes, edges)
            .and_then(|lineage| lineage.into_set(frame_interval, &TrackIdSource::new()))
            .map(Some)
    }

    pub fn frame_interval(&self) -> f32 {
        self.frame_interval
    }

    pub fn layout(&self) -> &TrackLayout {
        &self.layout
    }

    pub fn tracks(&self) -> &[Track] {
        &self.tracks
    }

    pub fn len(&self) -> usize {
        self.tracks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
    }

    pub fn node_count(&self) -> usize {
        self.detections.len()
    }

    pub fn links(&self) -> &[TrackLink] {
        &self.links
    }

    pub fn uncertain(&self, below: f64) -> impl Iterator<Item = TrackLink> + '_ {
        assert!(
            SolvedConfidence::new(below).is_some(),
            "an uncertainty threshold is a probability"
        );
        self.links
            .iter()
            .copied()
            .filter(move |link| link.confidence().value().is_some_and(|value| value < below))
    }

    pub fn confidence(&self, id: TrackId) -> Option<f64> {
        let mut weakest = None;
        for link in self.parents(id).chain(self.child_links(id)) {
            let value = link.confidence().value()?;
            weakest = Some(weakest.map_or(value, |held: f64| held.min(value)));
        }
        weakest
    }

    pub fn divisions(&self) -> usize {
        self.divisions
    }

    pub fn merges(&self) -> usize {
        self.merges
    }

    pub fn track(&self, id: TrackId) -> Option<&Track> {
        self.position(id).map(|slot| &self.tracks[slot])
    }

    pub(crate) fn position(&self, id: TrackId) -> Option<usize> {
        self.tracks.binary_search_by_key(&id, Track::id).ok()
    }
    pub fn contains(&self, id: TrackId) -> bool {
        self.position(id).is_some()
    }

    pub fn remapped(&self, mut centroid: impl FnMut(u32, ObjectId, [f32; 3]) -> [f32; 3]) -> Self {
        self.rebuilt(|node| {
            TrackNode::new(
                node.time(),
                node.id(),
                centroid(node.time(), node.id(), node.centroid()),
            )
        })
    }

    pub fn renumbered(&self, mut identity: impl FnMut(ObjectId) -> ObjectId) -> Self {
        self.rebuilt(|node| TrackNode::new(node.time(), identity(node.id()), node.centroid()))
    }

    pub fn reidentified(&self, ids: &[TrackId]) -> Self {
        assert_eq!(
            ids.len(),
            self.tracks.len(),
            "the assigned track ids cover every track"
        );
        let mut seen = HashSet::with_capacity(ids.len());
        assert!(
            ids.iter().all(|id| seen.insert(*id)),
            "the assigned track ids differ pairwise"
        );
        let mut rename = HashMap::with_capacity(ids.len());
        for (track, id) in self.tracks.iter().zip(ids) {
            rename.insert(track.id(), *id);
        }
        let tracks = self
            .tracks
            .iter()
            .zip(ids)
            .map(|(track, id)| track.reidentified(*id))
            .collect();
        let links = self
            .links
            .iter()
            .map(|link| {
                link.reidentified(
                    *rename
                        .get(&link.parent())
                        .expect("a track link joins the tracks of its set"),
                    *rename
                        .get(&link.child())
                        .expect("a track link joins the tracks of its set"),
                )
            })
            .collect();
        let source = self.source.clone();
        source.observe(&TrackIds::new(ids.to_vec()));
        Self::assembled(self.frame_interval, tracks, links, source)
            .expect("the assigned track ids keep the links")
    }

    pub fn inherited(&self, mut origin: impl FnMut(&Track) -> Option<TrackId>) -> Self {
        let mut chosen = Vec::with_capacity(self.tracks.len());
        let mut taken = HashSet::new();
        let mut fresh = Vec::new();
        for track in &self.tracks {
            match origin(track) {
                Some(id) if taken.insert(id) => chosen.push(Some(id)),
                _ => {
                    fresh.push(chosen.len());
                    chosen.push(None);
                }
            }
        }
        let inherited = TrackIds::new(chosen.iter().flatten().copied().collect::<Vec<_>>());
        self.source.observe(&inherited);
        let allocated = self.source.allocate(fresh.len());
        for (slot, id) in fresh.into_iter().zip(allocated.as_slice()) {
            chosen[slot] = Some(*id);
        }
        let ids = chosen
            .into_iter()
            .map(|id| id.expect("every track inherits or receives an id"))
            .collect::<Vec<_>>();
        self.reidentified(&ids)
    }

    fn rebuilt(&self, mut node_of: impl FnMut(TrackNode) -> TrackNode) -> Self {
        let tracks = self
            .tracks
            .iter()
            .map(|track| {
                Track::new(
                    track.id(),
                    track.nodes().iter().map(|node| node_of(*node)).collect(),
                )
            })
            .collect();
        Self::assembled(
            self.frame_interval,
            tracks,
            self.links.clone(),
            self.source.clone(),
        )
        .expect("a rebuilt track set keeps its links")
    }

    pub fn track_at(&self, time: u32, id: ObjectId) -> Option<TrackId> {
        self.detections
            .binary_search_by_key(&(time, id), |detection| (detection.0, detection.1))
            .ok()
            .map(|index| self.detections[index].2)
    }

    pub fn statistics(&self, id: TrackId) -> Option<TrackStatistics> {
        self.track(id)
            .map(|track| track.statistics(self.frame_interval))
    }

    pub fn parents(&self, id: TrackId) -> impl Iterator<Item = TrackLink> + '_ {
        let links = &self.links;
        let child = id.raw();
        let range = self
            .by_child
            .partition_point(move |at| links[*at as usize].child().raw() < child)
            ..self
                .by_child
                .partition_point(move |at| links[*at as usize].child().raw() <= child);
        self.by_child[range]
            .iter()
            .map(move |at| links[*at as usize])
    }

    pub fn parent(&self, id: TrackId) -> Option<TrackId> {
        self.parents(id).next().map(TrackLink::parent)
    }

    pub fn child_links(&self, id: TrackId) -> impl Iterator<Item = TrackLink> + '_ {
        let parent = id.raw();
        let range = self
            .links
            .partition_point(move |link| link.parent().raw() < parent)
            ..self
                .links
                .partition_point(move |link| link.parent().raw() <= parent);
        self.links[range].iter().copied()
    }

    pub fn children(&self, id: TrackId) -> impl Iterator<Item = TrackId> + '_ {
        self.child_links(id).map(TrackLink::child)
    }

    pub fn cut(&self, time: u32, id: ObjectId) -> Option<Self> {
        self.edit(|lineage| {
            let node = lineage.node_at(time, id)?;
            lineage.cut(node).then_some(())
        })
    }

    pub fn can_join(&self, source: (u32, ObjectId), target: (u32, ObjectId)) -> bool {
        target.0 > source.0 && self.ends_lineage(source) && self.begins_lineage(target)
    }

    pub fn joinable(
        &self,
        source: (u32, ObjectId),
        targets: &[Option<(u32, ObjectId)>],
    ) -> Vec<bool> {
        let open = self.ends_lineage(source);
        targets
            .iter()
            .map(|target| {
                open && target
                    .is_some_and(|target| target.0 > source.0 && self.begins_lineage(target))
            })
            .collect()
    }

    fn ends_lineage(&self, node: (u32, ObjectId)) -> bool {
        self.track_at(node.0, node.1).is_some_and(|track| {
            self.finishes_track(track, node) && self.child_links(track).next().is_none()
        })
    }

    fn begins_lineage(&self, node: (u32, ObjectId)) -> bool {
        self.track_at(node.0, node.1).is_some_and(|track| {
            self.begins_track(track, node) && self.parents(track).next().is_none()
        })
    }

    fn begins_track(&self, track: TrackId, node: (u32, ObjectId)) -> bool {
        self.track(track)
            .and_then(|track| track.nodes().first())
            .is_some_and(|first| (first.time(), first.id()) == node)
    }

    fn finishes_track(&self, track: TrackId, node: (u32, ObjectId)) -> bool {
        self.track(track)
            .and_then(|track| track.nodes().last())
            .is_some_and(|last| (last.time(), last.id()) == node)
    }

    pub fn join(&self, source: (u32, ObjectId), target: (u32, ObjectId)) -> Option<Self> {
        self.edit(|lineage| {
            let source = lineage.node_at(source.0, source.1)?;
            let target = lineage.node_at(target.0, target.1)?;
            lineage.can_join(source, target).then_some(())?;
            lineage.join(source, target);
            Some(())
        })
    }

    pub fn can_divide(
        &self,
        parent: (u32, ObjectId),
        first: (u32, ObjectId),
        second: (u32, ObjectId),
    ) -> bool {
        first != second
            && first.0 > parent.0
            && second.0 > parent.0
            && self.ends_lineage(parent)
            && self.begins_lineage(first)
            && self.begins_lineage(second)
    }

    pub fn divide(
        &self,
        parent: (u32, ObjectId),
        first: (u32, ObjectId),
        second: (u32, ObjectId),
    ) -> Option<Self> {
        self.edit(|lineage| {
            let parent = lineage.node_at(parent.0, parent.1)?;
            let first = lineage.node_at(first.0, first.1)?;
            let second = lineage.node_at(second.0, second.1)?;
            lineage.can_divide(parent, first, second).then_some(())?;
            lineage.divide(parent, first, second);
            Some(())
        })
    }

    pub fn can_merge(
        &self,
        first: (u32, ObjectId),
        second: (u32, ObjectId),
        child: (u32, ObjectId),
    ) -> bool {
        let lineage = Lineage::of(self);
        let (Some(first), Some(second), Some(child)) = (
            lineage.node_at(first.0, first.1),
            lineage.node_at(second.0, second.1),
            lineage.node_at(child.0, child.1),
        ) else {
            return false;
        };
        lineage.can_merge(first, second, child)
    }

    pub fn merge(
        &self,
        first: (u32, ObjectId),
        second: (u32, ObjectId),
        child: (u32, ObjectId),
    ) -> Option<Self> {
        self.edit(|lineage| {
            let first = lineage.node_at(first.0, first.1)?;
            let second = lineage.node_at(second.0, second.1)?;
            let child = lineage.node_at(child.0, child.1)?;
            lineage.can_merge(first, second, child).then_some(())?;
            lineage.merge(first, second, child);
            Some(())
        })
    }

    pub fn can_reparent(&self, child: (u32, ObjectId), parent: (u32, ObjectId)) -> bool {
        let Some(parent_track) = self.track_at(parent.0, parent.1) else {
            return false;
        };
        let Some(child_track) = self.track_at(child.0, child.1) else {
            return false;
        };
        child.0 > parent.0
            && self.finishes_track(parent_track, parent)
            && self.begins_track(child_track, child)
            && self.child_links(parent_track).count() < 2
            && self
                .child_links(parent_track)
                .all(|link| link.child() != child_track)
    }

    pub fn reparent(&self, child: (u32, ObjectId), parent: (u32, ObjectId)) -> Option<Self> {
        self.edit(|lineage| {
            let child = lineage.node_at(child.0, child.1)?;
            let parent = lineage.node_at(parent.0, parent.1)?;
            lineage.can_reparent(child, parent).then_some(())?;
            lineage.reparent(child, parent);
            Some(())
        })
    }

    pub fn can_detach(&self, child: (u32, ObjectId)) -> bool {
        self.track_at(child.0, child.1).is_some_and(|track| {
            self.begins_track(track, child) && self.parents(track).next().is_some()
        })
    }

    pub fn detach(&self, child: (u32, ObjectId)) -> Option<Self> {
        self.edit(|lineage| {
            let child = lineage.node_at(child.0, child.1)?;
            lineage.can_detach(child).then_some(())?;
            lineage.detach(child);
            Some(())
        })
    }

    pub fn without(&self, time: u32, id: ObjectId) -> Option<Self> {
        self.without_objects(time, &[id])
    }

    pub fn without_objects(&self, time: u32, ids: &[ObjectId]) -> Option<Self> {
        if ids.is_empty() {
            return None;
        }
        assert!(
            ids.windows(2).all(|pair| pair[0] < pair[1]),
            "removed track nodes are ordered and unique"
        );
        self.edit(|lineage| lineage.without_objects(time, ids).then_some(()))
    }

    pub fn append(&self, other: &Self) -> Self {
        assert_eq!(
            self.frame_interval, other.frame_interval,
            "a merged track set spans one frame interval"
        );
        let source = self.source.clone();
        let allocated = source.allocate(other.tracks.len());
        let mut rename = HashMap::with_capacity(other.tracks.len());
        for (track, id) in other.tracks.iter().zip(allocated.as_slice()) {
            rename.insert(track.id(), *id);
        }
        let mut tracks = self.tracks.clone();
        tracks.extend(
            other
                .tracks
                .iter()
                .zip(allocated.as_slice())
                .map(|(track, id)| track.reidentified(*id)),
        );
        let mut links = self.links.clone();
        links.extend(other.links.iter().map(|link| {
            link.reidentified(
                *rename
                    .get(&link.parent())
                    .expect("a track link joins the tracks of its set"),
                *rename
                    .get(&link.child())
                    .expect("a track link joins the tracks of its set"),
            )
        }));
        Self::assembled(self.frame_interval, tracks, links, source)
            .expect("a merged track set keeps its links")
    }

    pub fn mesh(&self, radius: f32) -> grana_geometry::Mesh {
        assert!(
            radius.is_finite() && radius > 0.0,
            "a track tube radius must be a positive length"
        );
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        for (slot, track) in self.tracks.iter().enumerate() {
            let object = slot as u32;
            let points = track
                .nodes()
                .iter()
                .map(|node| glam::Vec3::from(node.centroid()))
                .collect::<Vec<_>>();
            push(&mut vertices, &mut indices, object, &points, radius);
        }
        for link in &self.links {
            let parent = self
                .track(link.parent())
                .and_then(|track| track.nodes().last())
                .expect("the parent track holds nodes");
            let child = self
                .track(link.child())
                .and_then(|track| track.nodes().first())
                .expect("the child track holds nodes");
            let points = [
                glam::Vec3::from(parent.centroid()),
                glam::Vec3::from(child.centroid()),
            ];
            let slot = self
                .position(link.child())
                .expect("the child track lies in its set");
            push(&mut vertices, &mut indices, slot as u32, &points, radius);
        }
        grana_geometry::Mesh::new(vertices, indices)
    }

    fn edit(&self, mutate: impl FnOnce(&mut Lineage) -> Option<()>) -> Option<Self> {
        let mut lineage = Lineage::of(self);
        mutate(&mut lineage)?;
        lineage.into_set(self.frame_interval, &self.source).ok()
    }
}

fn push(
    vertices: &mut Vec<grana_geometry::MeshVertex>,
    indices: &mut Vec<u32>,
    object: u32,
    points: &[glam::Vec3],
    radius: f32,
) {
    let mut unique = Vec::with_capacity(points.len());
    for point in points {
        if unique
            .last()
            .is_none_or(|last: &glam::Vec3| last.distance_squared(*point) > 1.0e-12)
        {
            unique.push(*point);
        }
    }
    match unique.len() {
        0 => {}
        1 => grana_geometry::push_sphere(vertices, indices, unique[0].to_array(), radius, object),
        _ => {
            let positions = unique
                .iter()
                .map(|point| point.to_array())
                .collect::<Vec<_>>();
            let radii = vec![radius; positions.len()];
            grana_geometry::push_tube(vertices, indices, &positions, &radii, object);
        }
    }
}

fn distance(left: [f32; 3], right: [f32; 3]) -> f64 {
    (0..3)
        .map(|axis| f64::from(right[axis] - left[axis]).powi(2))
        .sum::<f64>()
        .sqrt()
}

pub fn grouped_tracks(
    frame_interval: f32,
    groups: impl IntoIterator<Item = (u32, u32, ObjectId, [f32; 3])>,
) -> Result<TrackSet, String> {
    let mut held = std::collections::BTreeMap::<u32, Vec<TrackNode>>::new();
    for (group, time, id, centroid) in groups {
        held.entry(group)
            .or_default()
            .push(TrackNode::new(time, id, centroid));
    }
    let mut tracks = Vec::new();
    for nodes in held.values_mut() {
        nodes.sort_unstable_by_key(|node| node.time());
        let mut start = 0;
        for index in 1..=nodes.len() {
            let broken = index == nodes.len() || nodes[index].time() != nodes[index - 1].time() + 1;
            if !broken {
                continue;
            }
            if index > start {
                let id = TrackId::FIRST
                    .after(tracks.len())
                    .expect("a grouped track lies inside the id space");
                tracks.push(Track::new(id, nodes[start..index].to_vec()));
            }
            start = index;
        }
    }
    TrackSet::try_with_links(frame_interval, tracks, Vec::new())
}
