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
        assert!(
            frame_interval.is_finite() && frame_interval > 0.0,
            "the frame interval is a positive duration"
        );
        let mut children = vec![0usize; tracks.len()];
        for (index, track) in tracks.iter().enumerate() {
            if let Some(parent) = track.parent() {
                let parent = parent as usize;
                assert!(parent < index, "a track follows its parent");
                assert!(
                    tracks[parent].end() < track.start(),
                    "a child track starts after its parent ends"
                );
                children[parent] += 1;
            }
        }
        for count in &children {
            assert!(
                *count == 0 || *count == 2,
                "a division produces exactly two daughter tracks"
            );
        }
        let divisions = children.iter().filter(|count| **count == 2).count();
        let mut links = Vec::new();
        for (index, track) in tracks.iter().enumerate() {
            for node in track.nodes() {
                links.push((node.time(), node.object(), index as u32));
            }
        }
        links.sort_unstable();
        assert!(
            links
                .windows(2)
                .all(|pair| (pair[0].0, pair[0].1) != (pair[1].0, pair[1].1)),
            "every detection belongs to one track"
        );
        Self {
            frame_interval,
            tracks,
            links,
            divisions,
        }
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
