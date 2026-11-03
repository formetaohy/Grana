use std::collections::{HashMap, HashSet};

use glam::Vec3;

const TRAIL_LIMIT: usize = 1 << 16;

pub(super) struct Cap {
    pub(super) positions: Vec<[f32; 3]>,
    pub(super) normals: Vec<[f32; 3]>,
    pub(super) triangles: Vec<u32>,
    pub(super) area: f64,
}

impl Cap {
    fn empty() -> Self {
        Self {
            positions: Vec::new(),
            normals: Vec::new(),
            triangles: Vec::new(),
            area: 0.0,
        }
    }

    fn push(&mut self, surface: &[[f32; 3]], trail: &[u32], sides: &[(u32, u32)]) {
        let corners = trail
            .iter()
            .map(|vertex| Vec3::from(surface[*vertex as usize]))
            .collect::<Vec<_>>();
        let centroid = corners.iter().copied().sum::<Vec3>() / corners.len() as f32;
        let mut normal = Vec3::ZERO;
        let mut area = 0.0f64;
        for side in sides {
            let first = Vec3::from(surface[side.1 as usize]);
            let second = Vec3::from(surface[side.0 as usize]);
            let cross = (first - centroid).cross(second - centroid);
            normal += cross;
            let size = 0.5 * cross.length();
            if size.is_finite() {
                area += f64::from(size);
            }
        }
        let normal = normal.normalize_or_zero();
        if normal == Vec3::ZERO {
            return;
        }
        let base = self.positions.len() as u32;
        self.positions.push(centroid.to_array());
        self.normals.push(normal.to_array());
        let mut slots = HashMap::with_capacity(trail.len());
        for vertex in trail {
            slots.entry(*vertex).or_insert_with(|| {
                let slot = self.positions.len() as u32;
                self.positions.push(surface[*vertex as usize]);
                self.normals.push(normal.to_array());
                slot
            });
        }
        for side in sides {
            let start = slots[&side.1];
            let end = slots[&side.0];
            if start == end {
                continue;
            }
            self.triangles.extend_from_slice(&[base, start, end]);
        }
        self.area += area;
    }
}

pub(super) fn build(positions: &[[f32; 3]], triangles: &[[u32; 3]], selected: &[usize]) -> Cap {
    let mut points: Vec<[f32; 3]> = Vec::new();
    let mut index: HashMap<[u32; 3], u32> = HashMap::new();
    let mut directed: HashMap<(u32, u32), u32> = HashMap::with_capacity(selected.len() * 3);
    for slot in selected {
        let triangle = triangles[*slot];
        let mut mapped = [0u32; 3];
        for (corner, vertex) in triangle.iter().enumerate() {
            let point = positions[*vertex as usize];
            mapped[corner] = *index.entry(point.map(f32::to_bits)).or_insert_with(|| {
                points.push(point);
                points.len() as u32 - 1
            });
        }
        for pair in [[0usize, 1usize], [1, 2], [2, 0]] {
            *directed
                .entry((mapped[pair[0]], mapped[pair[1]]))
                .or_insert(0) += 1;
        }
    }
    let mut open: HashMap<(u32, u32), (u32, u32)> = HashMap::new();
    for (&(from, to), &count) in &directed {
        if count == 1 && !directed.contains_key(&(to, from)) {
            open.insert((from.min(to), from.max(to)), (from, to));
        }
    }
    if open.is_empty() {
        return Cap::empty();
    }
    let mut adjacency: HashMap<u32, Vec<u32>> = HashMap::new();
    for key in open.keys() {
        adjacency.entry(key.0).or_default().push(key.1);
        adjacency.entry(key.1).or_default().push(key.0);
    }
    for list in adjacency.values_mut() {
        list.sort_unstable();
    }
    let mut starts = adjacency.keys().copied().collect::<Vec<_>>();
    starts.sort_unstable();
    let mut used: HashSet<(u32, u32)> = HashSet::new();
    let mut cap = Cap::empty();
    for start in starts {
        while let Some(target) = take(&mut adjacency, &mut used, start) {
            if target == start {
                continue;
            }
            let mut trail = vec![start, target];
            let mut current = target;
            while let Some(following) = take(&mut adjacency, &mut used, current) {
                if following == start || trail.len() > TRAIL_LIMIT {
                    break;
                }
                trail.push(following);
                current = following;
            }
            if trail.len() < 3 {
                continue;
            }
            let mut sides = Vec::with_capacity(trail.len());
            for index in 0..trail.len() {
                let first = trail[index];
                let second = trail[(index + 1) % trail.len()];
                let key = (first.min(second), first.max(second));
                if let Some(side) = open.get(&key) {
                    sides.push(*side);
                }
            }
            cap.push(&points, &trail, &sides);
        }
    }
    cap
}

fn take(
    adjacency: &mut HashMap<u32, Vec<u32>>,
    used: &mut HashSet<(u32, u32)>,
    vertex: u32,
) -> Option<u32> {
    let list = adjacency.get_mut(&vertex)?;
    let slot = list.iter().position(|target| {
        let key = (vertex.min(*target), vertex.max(*target));
        !used.contains(&key)
    })?;
    let target = list.remove(slot);
    used.insert((vertex.min(target), vertex.max(target)));
    Some(target)
}
