use crate::lineage::Lineage;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrackDetection {
    centroid: [f32; 3],
    radii: [f32; 3],
}

impl TrackDetection {
    pub fn new(centroid: [f32; 3], radii: [f32; 3]) -> Self {
        assert!(
            centroid.iter().all(|value| value.is_finite()),
            "a tracked detection lies at a finite centroid"
        );
        assert!(
            radii.iter().all(|value| value.is_finite() && *value > 0.0),
            "a tracked detection spans a positive radius on every axis"
        );
        Self { centroid, radii }
    }

    pub fn centroid(self) -> [f32; 3] {
        self.centroid
    }

    pub fn radii(self) -> [f32; 3] {
        self.radii
    }

    pub fn radius(self) -> f32 {
        (self.radii[0] * self.radii[1] * self.radii[2]).cbrt()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrackNode {
    time: u32,
    object: usize,
    centroid: [f32; 3],
}

impl TrackNode {
    pub fn new(time: u32, object: usize, centroid: [f32; 3]) -> Self {
        assert!(
            centroid.iter().all(|value| value.is_finite()),
            "a track node lies at a finite centroid"
        );
        Self {
            time,
            object,
            centroid,
        }
    }

    pub fn time(self) -> u32 {
        self.time
    }

    pub fn object(self) -> usize {
        self.object
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrackLink {
    parent: u32,
    child: u32,
    kind: Link,
}

impl TrackLink {
    pub fn new(parent: u32, child: u32, kind: Link) -> Self {
        assert!(parent != child, "a track link joins two tracks");
        Self {
            parent,
            child,
            kind,
        }
    }

    pub fn parent(self) -> u32 {
        self.parent
    }

    pub fn child(self) -> u32 {
        self.child
    }

    pub fn kind(self) -> Link {
        self.kind
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrackStatistics {
    frames: u32,
    duration: f64,
    length: f64,
    displacement: f64,
    speed: f64,
    straightness: f64,
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

    pub fn speed(self) -> f64 {
        self.speed
    }

    pub fn straightness(self) -> f64 {
        self.straightness
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Track {
    nodes: Vec<TrackNode>,
}

impl Track {
    pub fn new(nodes: Vec<TrackNode>) -> Self {
        assert!(!nodes.is_empty(), "a track holds at least one node");
        for pair in nodes.windows(2) {
            assert!(
                pair[1].time == pair[0].time + 1,
                "track nodes follow consecutive frames"
            );
        }
        Self { nodes }
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
        let speed = if duration > 0.0 {
            length / duration
        } else {
            0.0
        };
        let straightness = if length > 0.0 {
            (displacement / length).clamp(0.0, 1.0)
        } else {
            0.0
        };
        TrackStatistics {
            frames,
            duration,
            length,
            displacement,
            speed,
            straightness,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TrackSet {
    frame_interval: f32,
    tracks: Vec<Track>,
    links: Vec<TrackLink>,
    detections: Vec<(u32, usize, u32)>,
    divisions: usize,
    merges: usize,
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
        mut links: Vec<TrackLink>,
    ) -> Result<Self, String> {
        if !(frame_interval.is_finite() && frame_interval > 0.0) {
            return Err("the frame interval is a positive duration".to_string());
        }
        links.sort_unstable_by_key(|link| (link.parent, link.child));
        if links
            .windows(2)
            .any(|pair| (pair[0].parent, pair[0].child) == (pair[1].parent, pair[1].child))
        {
            return Err("a track link appears twice".to_string());
        }
        let mut detections = Vec::new();
        for (index, track) in tracks.iter().enumerate() {
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
                detections.push((node.time(), node.object(), index as u32));
            }
        }
        detections.sort_unstable();
        if detections
            .windows(2)
            .any(|pair| (pair[0].0, pair[0].1) == (pair[1].0, pair[1].1))
        {
            return Err("every detection belongs to one track".to_string());
        }
        let mut incoming = vec![0u8; tracks.len()];
        let mut outgoing = vec![0u8; tracks.len()];
        for link in &links {
            let parent = link.parent as usize;
            let child = link.child as usize;
            if parent >= tracks.len() || child >= tracks.len() {
                return Err("a track link names a missing track".to_string());
            }
            if parent >= child {
                return Err("a track link follows its parent".to_string());
            }
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
            let parent = link.parent as usize;
            let child = link.child as usize;
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
        Ok(Self {
            frame_interval,
            tracks,
            links,
            detections,
            divisions,
            merges,
        })
    }

    pub fn linked(
        frame_interval: f32,
        nodes: &[(u32, usize, [f32; 3])],
        edges: &[([u32; 2], [u32; 2])],
    ) -> Result<Option<Self>, String> {
        if nodes.is_empty() {
            return Ok(None);
        }
        Lineage::from_edges(nodes, edges)
            .and_then(|lineage| lineage.into_set(frame_interval))
            .map(Some)
    }

    pub fn frame_interval(&self) -> f32 {
        self.frame_interval
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

    pub fn divisions(&self) -> usize {
        self.divisions
    }

    pub fn merges(&self) -> usize {
        self.merges
    }

    pub fn track(&self, index: usize) -> Option<&Track> {
        self.tracks.get(index)
    }

    pub fn track_at(&self, time: u32, object: usize) -> Option<usize> {
        self.detections
            .binary_search_by_key(&(time, object), |detection| (detection.0, detection.1))
            .ok()
            .map(|index| self.detections[index].2 as usize)
    }

    pub fn statistics(&self, index: usize) -> Option<TrackStatistics> {
        self.track(index)
            .map(|track| track.statistics(self.frame_interval))
    }

    pub fn parents(&self, index: usize) -> impl Iterator<Item = TrackLink> + '_ {
        self.links
            .iter()
            .copied()
            .filter(move |link| link.child as usize == index)
    }

    pub fn parent(&self, index: usize) -> Option<usize> {
        self.parents(index).next().map(|link| link.parent as usize)
    }

    pub fn child_links(&self, index: usize) -> impl Iterator<Item = TrackLink> + '_ {
        self.links
            .iter()
            .copied()
            .filter(move |link| link.parent as usize == index)
    }

    pub fn children(&self, index: usize) -> impl Iterator<Item = usize> + '_ {
        self.child_links(index).map(|link| link.child as usize)
    }

    pub fn cut(&self, time: u32, object: usize) -> Option<Self> {
        self.edit(|lineage| {
            let node = lineage.node_at(time, object)?;
            lineage.cut(node).then_some(())
        })
    }

    pub fn can_join(&self, source: (u32, usize), target: (u32, usize)) -> bool {
        let lineage = Lineage::of(self);
        let (Some(source), Some(target)) = (
            lineage.node_at(source.0, source.1),
            lineage.node_at(target.0, target.1),
        ) else {
            return false;
        };
        lineage.can_join(source, target)
    }

    pub fn join(&self, source: (u32, usize), target: (u32, usize)) -> Option<Self> {
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
        parent: (u32, usize),
        first: (u32, usize),
        second: (u32, usize),
    ) -> bool {
        let lineage = Lineage::of(self);
        let (Some(parent), Some(first), Some(second)) = (
            lineage.node_at(parent.0, parent.1),
            lineage.node_at(first.0, first.1),
            lineage.node_at(second.0, second.1),
        ) else {
            return false;
        };
        lineage.can_divide(parent, first, second)
    }

    pub fn divide(
        &self,
        parent: (u32, usize),
        first: (u32, usize),
        second: (u32, usize),
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
        first: (u32, usize),
        second: (u32, usize),
        child: (u32, usize),
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
        first: (u32, usize),
        second: (u32, usize),
        child: (u32, usize),
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

    pub fn can_reparent(&self, child: (u32, usize), parent: (u32, usize)) -> bool {
        let lineage = Lineage::of(self);
        let (Some(child), Some(parent)) = (
            lineage.node_at(child.0, child.1),
            lineage.node_at(parent.0, parent.1),
        ) else {
            return false;
        };
        lineage.can_reparent(child, parent)
    }

    pub fn reparent(&self, child: (u32, usize), parent: (u32, usize)) -> Option<Self> {
        self.edit(|lineage| {
            let child = lineage.node_at(child.0, child.1)?;
            let parent = lineage.node_at(parent.0, parent.1)?;
            lineage.can_reparent(child, parent).then_some(())?;
            lineage.reparent(child, parent);
            Some(())
        })
    }

    pub fn can_detach(&self, child: (u32, usize)) -> bool {
        let lineage = Lineage::of(self);
        let Some(child) = lineage.node_at(child.0, child.1) else {
            return false;
        };
        lineage.can_detach(child)
    }

    pub fn detach(&self, child: (u32, usize)) -> Option<Self> {
        self.edit(|lineage| {
            let child = lineage.node_at(child.0, child.1)?;
            lineage.can_detach(child).then_some(())?;
            lineage.detach(child);
            Some(())
        })
    }

    pub fn without(&self, time: u32, object: usize) -> Option<Self> {
        self.without_objects(time, &[object])
    }

    pub fn without_objects(&self, time: u32, objects: &[usize]) -> Option<Self> {
        if objects.is_empty() {
            return None;
        }
        assert!(
            objects.windows(2).all(|pair| pair[0] < pair[1]),
            "removed track nodes are ordered and unique"
        );
        self.edit(|lineage| lineage.without_objects(time, objects).then_some(()))
    }

    pub fn shift(&self, offsets: &[usize]) -> Self {
        self.edit(|lineage| {
            lineage.shift(offsets);
            Some(())
        })
        .expect("shifting keeps every track")
    }

    pub fn append(&self, other: &Self) -> Self {
        assert_eq!(
            self.frame_interval, other.frame_interval,
            "a merged track set spans one frame interval"
        );
        let base = self.tracks.len() as u32;
        let mut tracks = self.tracks.clone();
        tracks.extend(other.tracks.iter().cloned());
        let mut links = self.links.clone();
        links.extend(
            other
                .links
                .iter()
                .map(|link| TrackLink::new(link.parent + base, link.child + base, link.kind)),
        );
        Self::with_links(self.frame_interval, tracks, links)
    }

    pub fn mesh(&self, radius: f32) -> grana_geometry::Mesh {
        assert!(
            radius.is_finite() && radius > 0.0,
            "a track tube radius must be a positive length"
        );
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        for (index, track) in self.tracks.iter().enumerate() {
            let object = index as u32;
            let points = track
                .nodes()
                .iter()
                .map(|node| glam::Vec3::from(node.centroid()))
                .collect::<Vec<_>>();
            push(&mut vertices, &mut indices, object, &points, radius);
        }
        for link in &self.links {
            let parent = self
                .track(link.parent as usize)
                .and_then(|track| track.nodes().last())
                .expect("the parent track holds nodes");
            let child = self
                .track(link.child as usize)
                .and_then(|track| track.nodes().first())
                .expect("the child track holds nodes");
            let points = [
                glam::Vec3::from(parent.centroid()),
                glam::Vec3::from(child.centroid()),
            ];
            push(&mut vertices, &mut indices, link.child, &points, radius);
        }
        grana_geometry::Mesh::new(vertices, indices)
    }

    fn edit(&self, mutate: impl FnOnce(&mut Lineage) -> Option<()>) -> Option<Self> {
        let mut lineage = Lineage::of(self);
        mutate(&mut lineage)?;
        lineage.into_set(self.frame_interval).ok()
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
