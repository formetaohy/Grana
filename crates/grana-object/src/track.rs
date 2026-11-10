use crate::graph::TrackGraph;

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
    parent: Option<u32>,
    nodes: Vec<TrackNode>,
}

impl Track {
    pub fn new(parent: Option<u32>, nodes: Vec<TrackNode>) -> Self {
        assert!(!nodes.is_empty(), "a track holds at least one node");
        for pair in nodes.windows(2) {
            assert!(
                pair[1].time > pair[0].time,
                "track nodes follow the time axis"
            );
        }
        Self { parent, nodes }
    }

    pub fn parent(&self) -> Option<u32> {
        self.parent
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
        let frames = last.time - first.time + 1;
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
    links: Vec<(u32, usize, u32)>,
    divisions: usize,
}

impl TrackSet {
    pub fn new(frame_interval: f32, tracks: Vec<Track>) -> Self {
        match Self::try_new(frame_interval, tracks) {
            Ok(set) => set,
            Err(detail) => panic!("{detail}"),
        }
    }

    pub fn try_new(frame_interval: f32, tracks: Vec<Track>) -> Result<Self, String> {
        if !(frame_interval.is_finite() && frame_interval > 0.0) {
            return Err("the frame interval is a positive duration".to_string());
        }
        let mut children = vec![0usize; tracks.len()];
        for (index, track) in tracks.iter().enumerate() {
            if track.nodes().is_empty() {
                return Err("a track holds at least one node".to_string());
            }
            if track
                .nodes()
                .windows(2)
                .any(|pair| pair[1].time() <= pair[0].time())
            {
                return Err("track nodes follow the time axis".to_string());
            }
            if let Some(parent) = track.parent() {
                let parent = parent as usize;
                if parent >= index {
                    return Err("a track follows its parent".to_string());
                }
                if tracks[parent].end() >= track.start() {
                    return Err("a child track starts after its parent ends".to_string());
                }
                children[parent] += 1;
            }
        }
        for count in &children {
            if !(*count == 0 || *count == 2) {
                return Err("a division produces exactly two daughter tracks".to_string());
            }
        }
        let divisions = children.iter().filter(|count| **count == 2).count();
        let mut links = Vec::new();
        for (index, track) in tracks.iter().enumerate() {
            for node in track.nodes() {
                links.push((node.time(), node.object(), index as u32));
            }
        }
        links.sort_unstable();
        if links
            .windows(2)
            .any(|pair| (pair[0].0, pair[0].1) == (pair[1].0, pair[1].1))
        {
            return Err("every detection belongs to one track".to_string());
        }
        Ok(Self {
            frame_interval,
            tracks,
            links,
            divisions,
        })
    }

    pub fn frame_interval(&self) -> f32 {
        self.frame_interval
    }

    /// Builds the set from the edges a file records between detections: every edge joins a node of
    /// an earlier frame to a node of a later one, and a node follows at most one predecessor and
    /// leads to at most two daughters. `nodes` lists every detection as `(time, object)` together
    /// with the centroid the track tube passes through.
    pub fn linked(
        frame_interval: f32,
        nodes: &[(u32, usize, [f32; 3])],
        edges: &[([u32; 2], [u32; 2])],
    ) -> Result<Option<Self>, String> {
        if nodes.is_empty() {
            return Ok(None);
        }
        let mut slots = std::collections::HashMap::with_capacity(nodes.len());
        for (slot, (time, object, _)) in nodes.iter().enumerate() {
            if slots.insert((*time, *object), slot).is_some() {
                return Err(format!(
                    "the detection {object} of the frame {time} appears twice"
                ));
            }
        }
        let mut successors: Vec<Vec<usize>> = vec![Vec::new(); nodes.len()];
        let mut predecessors: Vec<Option<usize>> = vec![None; nodes.len()];
        for (source, target) in edges {
            let (Some(source), Some(target)) = (
                slots.get(&(source[0], source[1] as usize)).copied(),
                slots.get(&(target[0], target[1] as usize)).copied(),
            ) else {
                continue;
            };
            if source == target || nodes[source].0 >= nodes[target].0 {
                continue;
            }
            if let Some(previous) = predecessors[target]
                && previous != source
            {
                return Err(format!(
                    "the detection {} of the frame {} follows both {} and {}",
                    target, nodes[target].0, previous, source
                ));
            }
            predecessors[target] = Some(source);
            if !successors[source].contains(&target) {
                successors[source].push(target);
            }
        }
        for next in &successors {
            if next.len() > 2 {
                return Err(format!("a detection links to {} daughters", next.len()));
            }
        }
        let mut tracks: Vec<Track> = Vec::new();
        let mut visited = vec![false; nodes.len()];
        let mut pending = predecessors
            .iter()
            .enumerate()
            .filter(|(_, previous)| previous.is_none())
            .map(|(slot, _)| (slot, None))
            .collect::<Vec<_>>();
        pending.reverse();
        while let Some((start, parent)) = pending.pop() {
            let mut held = Vec::new();
            let mut current = start;
            let mut daughters = Vec::new();
            loop {
                if visited[current] {
                    return Err("a detection belongs to two tracks".to_string());
                }
                visited[current] = true;
                let (time, object, centroid) = nodes[current];
                held.push(TrackNode::new(time, object, centroid));
                match successors[current].as_slice() {
                    [] => break,
                    [only] => current = *only,
                    [first, second] => {
                        daughters = vec![*first, *second];
                        break;
                    }
                    _ => unreachable!("a detection links to at most two daughters"),
                }
            }
            let index = tracks.len() as u32;
            tracks.push(Track::new(parent, held));
            daughters.reverse();
            for daughter in daughters {
                pending.push((daughter, Some(index)));
            }
        }
        if visited.iter().any(|visited| !visited) {
            return Err("the track edges close a cycle".to_string());
        }
        Self::try_new(frame_interval, tracks).map(Some)
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
        self.links.len()
    }

    pub fn divisions(&self) -> usize {
        self.divisions
    }

    pub fn track(&self, index: usize) -> Option<&Track> {
        self.tracks.get(index)
    }

    pub fn track_at(&self, time: u32, object: usize) -> Option<usize> {
        self.links
            .binary_search_by_key(&(time, object), |link| (link.0, link.1))
            .ok()
            .map(|index| self.links[index].2 as usize)
    }

    pub fn statistics(&self, index: usize) -> Option<TrackStatistics> {
        self.track(index)
            .map(|track| track.statistics(self.frame_interval))
    }

    pub fn children(&self, index: usize) -> impl Iterator<Item = usize> + '_ {
        self.tracks
            .iter()
            .enumerate()
            .filter(move |(_, track)| track.parent() == Some(index as u32))
            .map(|(child, _)| child)
    }

    pub fn cut(&self, time: u32, object: usize) -> Option<Self> {
        let mut graph = TrackGraph::from_set(self);
        let node = graph.node_at(time, object)?;
        if !graph.cut(node) {
            return None;
        }
        Some(graph.into_set(self.frame_interval))
    }

    pub fn can_join(&self, source: (u32, usize), target: (u32, usize)) -> bool {
        let Some(source_track) = self.track_at(source.0, source.1) else {
            return false;
        };
        let Some(target_track) = self.track_at(target.0, target.1) else {
            return false;
        };
        let Some(track) = self.track(source_track) else {
            return false;
        };
        let Some(last) = track.nodes().last() else {
            return false;
        };
        if (last.time(), last.object()) != source || self.children(source_track).next().is_some() {
            return false;
        }
        let Some(track) = self.track(target_track) else {
            return false;
        };
        let Some(first) = track.nodes().first() else {
            return false;
        };
        (first.time(), first.object()) == target && track.parent().is_none() && target.0 > source.0
    }

    pub fn join(&self, source: (u32, usize), target: (u32, usize)) -> Option<Self> {
        let mut graph = TrackGraph::from_set(self);
        let source = graph.node_at(source.0, source.1)?;
        let target = graph.node_at(target.0, target.1)?;
        if !graph.can_join(source, target) {
            return None;
        }
        graph.join(source, target);
        Some(graph.into_set(self.frame_interval))
    }

    pub fn without(&self, time: u32, object: usize) -> Option<Self> {
        self.without_objects(time, &[object])
    }

    pub fn shift(&self, offsets: &[usize]) -> Self {
        let tracks = self
            .tracks
            .iter()
            .map(|track| {
                let nodes = track
                    .nodes()
                    .iter()
                    .map(|node| {
                        let offset = offsets.get(node.time() as usize).copied().unwrap_or(0);
                        TrackNode::new(node.time(), node.object() + offset, node.centroid())
                    })
                    .collect::<Vec<_>>();
                Track::new(track.parent(), nodes)
            })
            .collect::<Vec<_>>();
        Self::new(self.frame_interval, tracks)
    }

    pub fn append(&self, other: &Self) -> Self {
        assert_eq!(
            self.frame_interval, other.frame_interval,
            "a merged track set spans one frame interval"
        );
        let base = self.tracks.len() as u32;
        let mut tracks = self.tracks.clone();
        tracks.extend(other.tracks.iter().map(|track| {
            Track::new(
                track.parent().map(|parent| parent + base),
                track.nodes().to_vec(),
            )
        }));
        Self::new(self.frame_interval, tracks)
    }

    pub fn without_objects(&self, time: u32, objects: &[usize]) -> Option<Self> {
        if objects.is_empty() {
            return None;
        }
        assert!(
            objects.windows(2).all(|pair| pair[0] < pair[1]),
            "removed track nodes are ordered and unique"
        );
        let mut changed = false;
        let mut tracks = Vec::with_capacity(self.tracks.len());
        for track in &self.tracks {
            let mut nodes = Vec::with_capacity(track.nodes().len());
            for node in track.nodes().iter().copied() {
                let removed = if node.time() == time {
                    objects.partition_point(|object| *object < node.object())
                } else {
                    0
                };
                if node.time() == time && objects.get(removed) == Some(&node.object()) {
                    changed = true;
                    continue;
                }
                changed |= removed > 0;
                nodes.push(TrackNode::new(
                    node.time(),
                    node.object() - removed,
                    node.centroid(),
                ));
            }
            tracks.push((track.parent(), nodes));
        }
        if !changed {
            return None;
        }
        let mut index = vec![None; tracks.len()];
        let mut kept = Vec::with_capacity(tracks.len());
        for (old, (parent, nodes)) in tracks.into_iter().enumerate() {
            if nodes.is_empty() {
                continue;
            }
            index[old] = Some(kept.len());
            kept.push((parent, nodes));
        }
        let mut parents = kept
            .iter()
            .map(|(parent, _)| parent.and_then(|parent| index[parent as usize]))
            .collect::<Vec<_>>();
        loop {
            let mut children = vec![0usize; kept.len()];
            for parent in parents.iter().flatten() {
                children[*parent] += 1;
            }
            let mut detached = false;
            for parent in parents.iter_mut() {
                if let Some(slot) = *parent
                    && children[slot] != 2
                {
                    *parent = None;
                    detached = true;
                }
            }
            if !detached {
                break;
            }
        }
        let tracks = kept
            .into_iter()
            .zip(parents)
            .map(|((_, nodes), parent)| Track::new(parent.map(|parent| parent as u32), nodes))
            .collect::<Vec<_>>();
        Some(Self::new(self.frame_interval, tracks))
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
            let mut points = Vec::with_capacity(track.nodes().len() + 1);
            if let Some(parent) = track.parent() {
                let parent = self
                    .track(parent as usize)
                    .expect("the parent track exists");
                let end = parent.nodes().last().expect("the parent holds nodes");
                points.push(glam::Vec3::from(end.centroid()));
            }
            points.extend(
                track
                    .nodes()
                    .iter()
                    .map(|node| glam::Vec3::from(node.centroid())),
            );
            deduplicate(&mut points);
            match points.len() {
                0 => {}
                1 => grana_geometry::push_sphere(
                    &mut vertices,
                    &mut indices,
                    points[0].to_array(),
                    radius,
                    object,
                ),
                _ => {
                    let positions = points
                        .iter()
                        .map(|point| point.to_array())
                        .collect::<Vec<_>>();
                    let radii = vec![radius; positions.len()];
                    grana_geometry::push_tube(
                        &mut vertices,
                        &mut indices,
                        &positions,
                        &radii,
                        object,
                    );
                }
            }
        }
        grana_geometry::Mesh::new(vertices, indices)
    }
}

fn deduplicate(points: &mut Vec<glam::Vec3>) {
    let mut unique = Vec::with_capacity(points.len());
    for point in points.drain(..) {
        if unique
            .last()
            .is_none_or(|last: &glam::Vec3| last.distance_squared(point) > 1.0e-12)
        {
            unique.push(point);
        }
    }
    *points = unique;
}

fn distance(left: [f32; 3], right: [f32; 3]) -> f64 {
    (0..3)
        .map(|axis| f64::from(right[axis] - left[axis]).powi(2))
        .sum::<f64>()
        .sqrt()
}
