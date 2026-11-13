pub use grana_object::ClusterSpec;

use std::collections::HashMap;

use super::LocalizationFrame;

const CELL_LIMIT: usize = 27;

const PLANAR_LIMIT: usize = 9;

const STEP: usize = 1 << 16;

const ESTIMATE: usize = 65_536;

const GAP_LIMIT: u32 = 64;

const GAP_SHARE: u32 = 20;

const UNGROUPED: u32 = u32::MAX;

#[derive(Clone, Debug, PartialEq)]
pub struct Clusters {
    groups: Vec<Vec<Option<u32>>>,
    molecules: u32,
    localizations: u64,
}

impl Clusters {
    pub fn groups(&self) -> &[Vec<Option<u32>>] {
        &self.groups
    }

    pub fn molecules(&self) -> u32 {
        self.molecules
    }

    pub fn localizations(&self) -> u64 {
        self.localizations
    }
}

pub fn cluster(
    frames: &[LocalizationFrame<'_>],
    spec: ClusterSpec,
    grouped: bool,
    report: &mut dyn FnMut(usize, usize) -> bool,
) -> Result<Option<Clusters>, String> {
    if frames.is_empty() {
        return Err("the frame range holds no frame".to_string());
    }
    let total = frames.len();
    let mut points = Vec::new();
    let mut groups = Vec::with_capacity(total);
    for (time, frame) in frames.iter().enumerate() {
        for (index, spot) in frame.spots().iter().enumerate() {
            if !frame.visible(index) {
                continue;
            }
            points.push(Point {
                center: spot.center(),
                time: time as u32,
                slot: index as u32,
                group: spot.group().unwrap_or(UNGROUPED),
            });
        }
        groups.push(vec![None; frame.spots().len()]);
        if !report(time + 1, total + 1) {
            return Ok(None);
        }
    }
    if points.is_empty() {
        return Ok(Some(Clusters {
            groups,
            molecules: 0,
            localizations: 0,
        }));
    }
    let cell = f64::from(spec.radius());
    let reach = cell * cell;
    let mut grid = HashMap::<[i64; 3], Vec<u32>>::new();
    for (index, point) in points.iter().enumerate() {
        grid.entry(key(&point.center, cell))
            .or_default()
            .push(index as u32);
    }
    let mut forest = Forest::new(points.len());
    let mut neighbours = Vec::new();
    for (index, point) in points.iter().enumerate() {
        let at = key(&point.center, cell);
        neighbours.clear();
        for offset in OFFSETS {
            let Some(held) = grid.get(&[
                at[0] + i64::from(offset[0]),
                at[1] + i64::from(offset[1]),
                at[2] + i64::from(offset[2]),
            ]) else {
                continue;
            };
            neighbours.extend(held.iter().copied().filter(|other| *other as usize > index));
        }
        for other in neighbours.iter().copied() {
            let candidate = &points[other as usize];
            if candidate.time.abs_diff(point.time) > spec.dark() {
                continue;
            }
            if grouped && candidate.group != point.group {
                continue;
            }
            let distance = [0, 1, 2]
                .map(|axis| {
                    let delta = f64::from(candidate.center[axis]) - f64::from(point.center[axis]);
                    delta * delta
                })
                .into_iter()
                .sum::<f64>();
            if distance <= reach {
                forest.union(index as u32, other);
            }
        }
        if index % STEP == STEP - 1 && !report(total, total + 1) {
            return Ok(None);
        }
    }
    let mut sizes = HashMap::<u32, u32>::with_capacity(points.len());
    for index in 0..points.len() as u32 {
        let root = forest.find(index);
        *sizes.entry(root).or_default() += 1;
    }
    let mut assigned = HashMap::<u32, u32>::new();
    let mut localizations = 0u64;
    for (index, point) in points.iter().enumerate() {
        let root = forest.find(index as u32);
        if sizes.get(&root).copied().unwrap_or(0) < spec.minimum() {
            continue;
        }
        let next = assigned.len() as u32 + 1;
        let group = *assigned.entry(root).or_insert(next);
        groups[point.time as usize][point.slot as usize] = Some(group);
        localizations += 1;
    }
    if !report(total + 1, total + 1) {
        return Ok(None);
    }
    Ok(Some(Clusters {
        groups,
        molecules: assigned.len() as u32,
        localizations,
    }))
}

pub(super) fn estimate(frames: &[LocalizationFrame<'_>], precision: f32) -> ClusterSpec {
    assert!(
        precision.is_finite() && precision > 0.0,
        "a localization precision is a positive length"
    );
    let radius = (2.0 * precision).clamp(ClusterSpec::RADIUS_MINIMUM, ClusterSpec::RADIUS_MAXIMUM);
    let dark = dark_frames(frames, radius);
    ClusterSpec::new(radius, dark, ClusterSpec::DEFAULT_MINIMUM)
}

fn dark_frames(frames: &[LocalizationFrame<'_>], radius: f32) -> u32 {
    let total = frames.iter().map(LocalizationFrame::count).sum::<usize>();
    let stride = (total / ESTIMATE).max(1);
    let mut points = Vec::with_capacity(total.div_ceil(stride).min(ESTIMATE) + 1);
    let mut seen = 0usize;
    for (time, frame) in frames.iter().enumerate() {
        for (index, spot) in frame.spots().iter().enumerate() {
            if !frame.visible(index) {
                continue;
            }
            if seen.is_multiple_of(stride) {
                let center = spot.center();
                points.push(Sample {
                    center: [center[0], center[1]],
                    time: time as u32,
                });
            }
            seen += 1;
        }
    }
    if points.len() < 2 {
        return ClusterSpec::DEFAULT_DARK;
    }
    let cell = f64::from(radius);
    let reach = cell * cell;
    let mut grid = HashMap::<[i64; 2], Vec<u32>>::new();
    for (index, point) in points.iter().enumerate() {
        grid.entry(planar(&point.center, cell))
            .or_default()
            .push(index as u32);
    }
    let mut forest = Forest::new(points.len());
    let mut neighbours = Vec::new();
    for (index, point) in points.iter().enumerate() {
        let at = planar(&point.center, cell);
        neighbours.clear();
        for offset in PLANAR {
            let Some(held) =
                grid.get(&[at[0] + i64::from(offset[0]), at[1] + i64::from(offset[1])])
            else {
                continue;
            };
            neighbours.extend(held.iter().copied().filter(|other| *other as usize > index));
        }
        for other in neighbours.iter().copied() {
            let candidate = &points[other as usize];
            let distance = [0, 1].map(|axis| {
                let delta = f64::from(candidate.center[axis]) - f64::from(point.center[axis]);
                delta * delta
            });
            if distance[0] + distance[1] <= reach {
                forest.union(index as u32, other);
            }
        }
    }
    let mut held = vec![Vec::<u32>::new(); points.len()];
    for (index, point) in points.iter().enumerate() {
        let root = forest.find(index as u32);
        held[root as usize].push(point.time);
    }
    let mut gaps = [0u32; GAP_LIMIT as usize + 1];
    for times in held.iter_mut().filter(|times| times.len() > 1) {
        times.sort_unstable();
        times.dedup();
        for pair in times.windows(2) {
            let gap = pair[1] - pair[0] - 1;
            if (1..=GAP_LIMIT).contains(&gap) {
                gaps[gap as usize] += 1;
            }
        }
    }
    let first = gaps[1];
    if first == 0 {
        return 1;
    }
    let mut dark = 1u32;
    for (gap, count) in gaps.iter().enumerate().skip(2) {
        if count * GAP_SHARE >= first {
            dark = gap as u32;
        }
    }
    dark
}

struct Point {
    center: [f32; 3],
    time: u32,
    slot: u32,
    group: u32,
}

struct Sample {
    center: [f32; 2],
    time: u32,
}

const OFFSETS: [[i8; 3]; CELL_LIMIT] = offsets();

const PLANAR: [[i8; 2]; PLANAR_LIMIT] = planar_offsets();

const fn offsets() -> [[i8; 3]; CELL_LIMIT] {
    let mut held = [[0i8; 3]; CELL_LIMIT];
    let mut at = 0usize;
    let mut x = -1i8;
    while x <= 1 {
        let mut y = -1i8;
        while y <= 1 {
            let mut z = -1i8;
            while z <= 1 {
                held[at] = [x, y, z];
                at += 1;
                z += 1;
            }
            y += 1;
        }
        x += 1;
    }
    held
}

const fn planar_offsets() -> [[i8; 2]; PLANAR_LIMIT] {
    let mut held = [[0i8; 2]; PLANAR_LIMIT];
    let mut at = 0usize;
    let mut x = -1i8;
    while x <= 1 {
        let mut y = -1i8;
        while y <= 1 {
            held[at] = [x, y];
            at += 1;
            y += 1;
        }
        x += 1;
    }
    held
}

fn key(center: &[f32; 3], cell: f64) -> [i64; 3] {
    center.map(|value| (f64::from(value) / cell).floor() as i64)
}

fn planar(center: &[f32; 2], cell: f64) -> [i64; 2] {
    center.map(|value| (f64::from(value) / cell).floor() as i64)
}

struct Forest {
    parent: Vec<u32>,
    rank: Vec<u8>,
}

impl Forest {
    fn new(count: usize) -> Self {
        Self {
            parent: (0..count as u32).collect(),
            rank: vec![0; count],
        }
    }

    fn find(&mut self, mut index: u32) -> u32 {
        while self.parent[index as usize] != index {
            let grand = self.parent[self.parent[index as usize] as usize];
            self.parent[index as usize] = grand;
            index = grand;
        }
        index
    }

    fn union(&mut self, left: u32, right: u32) {
        let left = self.find(left);
        let right = self.find(right);
        if left == right {
            return;
        }
        let (heavy, light) = if self.rank[left as usize] >= self.rank[right as usize] {
            (left, right)
        } else {
            (right, left)
        };
        self.parent[light as usize] = heavy;
        if self.rank[heavy as usize] == self.rank[light as usize] {
            self.rank[heavy as usize] += 1;
        }
    }
}
