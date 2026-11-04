use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use glam::Vec3;
use grana_geometry::{Bounds, GeometryStore, Mesh, MeshVertex};
use grana_object::{
    Basin, IntensityTable, SampleAccumulator, SampleStatistics, SurfaceSet, SurfaceStatistics,
    SurfaceTopology,
};

use super::cap;
use crate::schedule;
use grana_object::SurfaceCut;

const EMPTY: u32 = u32::MAX;
const SPLIT_FLOOR: f64 = 1.0;
const MAX_FOLD: u32 = 8;

fn cancelled(cancel: Option<&AtomicBool>) -> bool {
    cancel.is_some_and(|cancel| cancel.load(Ordering::Relaxed))
}

pub(super) fn cut(
    topology: &SurfaceTopology,
    spec: SurfaceCut,
    removed: &[u32],
    workers: usize,
    store: Option<&Arc<GeometryStore>>,
    cancel: Option<&AtomicBool>,
) -> Result<Option<SurfaceSet>, String> {
    let info = topology.info();
    let seeded = info.seeds() > 0;
    let mut merged = Merged::new(topology, removed);
    merged.close(spec.split(), cancel);
    if cancelled(cancel) {
        return Ok(None);
    }
    let targets = merged.targets();
    merged.check_seeds(&targets)?;
    let (order, offsets) = grouped(topology, &targets);
    let candidates = (0..topology.basins().len() as u32)
        .filter(|root| {
            !merged.removed[*root as usize]
                && merged.find(*root) == *root
                && merged.nodes[*root as usize].voxels() > 0
                && offsets[*root as usize + 1] > offsets[*root as usize]
                && (!seeded || merged.nodes[*root as usize].seed().is_some())
        })
        .collect::<Vec<_>>();
    let mut leaves_of = vec![Vec::new(); topology.basins().len()];
    for (basin, target) in targets.iter().enumerate() {
        if *target != EMPTY {
            leaves_of[*target as usize].push(basin as u32);
        }
    }
    let meshed = Meshed {
        topology,
        merged: &merged,
        spec,
    };
    let chunks = schedule::map(&candidates, workers, |root| {
        if cancelled(cancel) {
            return None;
        }
        let range = offsets[*root as usize] as usize..offsets[*root as usize + 1] as usize;
        meshed.chunk(*root, &order[range])
    });
    if cancelled(cancel) {
        return Ok(None);
    }
    let mut objects = Vec::new();
    let mut leaves = Vec::new();
    let mut intensities = Vec::new();
    let mut kept = Vec::new();
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for (root, chunk) in candidates.into_iter().zip(chunks) {
        let Some(chunk) = chunk else {
            continue;
        };
        let object = objects.len() as u32;
        let base = vertices.len() as u32;
        for (position, normal) in &chunk.vertices {
            vertices.push(MeshVertex::new(*position, *normal, object));
        }
        indices.extend(chunk.indices.iter().map(|index| base + index));
        objects.push(chunk.statistics);
        leaves.push(std::mem::take(&mut leaves_of[root as usize]));
        if let Some(seed) = chunk.seed {
            kept.push(seed);
        }
        intensities.extend(chunk.intensities);
    }
    if kept.len() != info.seeds() {
        let missing = (0..info.seeds())
            .find(|seed| !kept.contains(&(*seed as u32)))
            .expect("the kept seeds cover fewer seed points");
        return Err(format!(
            "the seed point {} no longer marks an object of the detected surface",
            missing + 1
        ));
    }
    let mesh = Mesh::new(vertices, indices);
    let intensities = IntensityTable::new(info.channels(), intensities);
    let removed = removed.to_vec();
    let set = match store {
        Some(store) => SurfaceSet::paged(info.level(), store, mesh, objects, intensities)
            .map(|set| set.with_cut(leaves, removed))
            .map_err(|error| error.to_string())?,
        None => SurfaceSet::new(info.level(), mesh, objects, intensities).with_cut(leaves, removed),
    };
    Ok(Some(set))
}

fn grouped(topology: &SurfaceTopology, targets: &[u32]) -> (Vec<u32>, Vec<u32>) {
    let patches = topology.patches();
    let roots = topology.basins().len();
    let mut offsets = vec![0u32; roots + 1];
    for basin in patches.basins() {
        let target = targets[*basin as usize];
        if target != EMPTY {
            offsets[target as usize + 1] += 1;
        }
    }
    for slot in 1..=roots {
        offsets[slot] += offsets[slot - 1];
    }
    let mut cursor = offsets.clone();
    let mut order = vec![0u32; offsets[roots] as usize];
    for (index, basin) in patches.basins().iter().enumerate() {
        let target = targets[*basin as usize];
        if target == EMPTY {
            continue;
        }
        let slot = cursor[target as usize];
        order[slot as usize] = index as u32;
        cursor[target as usize] += 1;
    }
    (order, offsets)
}

struct Merged<'a> {
    topology: &'a SurfaceTopology,
    nodes: Vec<Basin>,
    accumulators: Vec<SampleAccumulator>,
    parent: Vec<u32>,
    rank: Vec<u8>,
    channels: usize,
    removed: Vec<bool>,
}

impl<'a> Merged<'a> {
    fn new(topology: &'a SurfaceTopology, removed: &[u32]) -> Self {
        let nodes = topology.basins().to_vec();
        let parent = (0..nodes.len() as u32).collect();
        let rank = vec![0; nodes.len()];
        let mut excluded = vec![false; nodes.len()];
        for basin in removed {
            assert!(
                (*basin as usize) < nodes.len(),
                "a removed surface names a watershed basin"
            );
            excluded[*basin as usize] = true;
        }
        Self {
            topology,
            nodes,
            accumulators: topology.accumulators().to_vec(),
            parent,
            rank,
            channels: topology.info().channels(),
            removed: excluded,
        }
    }
    fn find(&mut self, node: u32) -> u32 {
        let mut root = node;
        while self.parent[root as usize] != root {
            root = self.parent[root as usize];
        }
        let mut current = node;
        while self.parent[current as usize] != root {
            let next = self.parent[current as usize];
            self.parent[current as usize] = root;
            current = next;
        }
        root
    }

    fn close(&mut self, split: f32, cancel: Option<&AtomicBool>) {
        let seeded = self.topology.info().seeds() > 0;
        let merge_all = split == 0.0 && !seeded;
        let mut seams = HashMap::<(u32, u32), u32>::new();
        for seam in self.topology.seams() {
            let slot = seams.entry((seam.left(), seam.right())).or_insert(0);
            *slot = (*slot).max(seam.saddle());
        }
        let mut seams = seams.into_iter().collect::<Vec<_>>();
        seams.sort_unstable_by(|(left_key, left_saddle), (right_key, right_saddle)| {
            right_saddle.cmp(left_saddle).then(left_key.cmp(right_key))
        });
        for (index, ((left, right), saddle)) in seams.into_iter().enumerate() {
            if index.is_multiple_of(4096) && cancelled(cancel) {
                return;
            }
            self.join(left, right, saddle, split, merge_all, seeded);
        }
    }

    fn join(
        &mut self,
        left: u32,
        right: u32,
        saddle: u32,
        split: f32,
        merge_all: bool,
        seeded: bool,
    ) {
        let left = self.find(left);
        let right = self.find(right);
        if left == right || self.removed[left as usize] || self.removed[right as usize] {
            return;
        }
        if merge_all {
            self.merge(left, right);
            return;
        }
        let seeds = (
            self.nodes[left as usize].seed(),
            self.nodes[right as usize].seed(),
        );
        match seeds {
            (Some(first), Some(second)) if first != second => return,
            (Some(_), _) | (_, Some(_)) if seeded => {
                self.merge(left, right);
                return;
            }
            _ => {}
        }
        let peak = f64::from(
            self.nodes[left as usize]
                .reach()
                .min(self.nodes[right as usize].reach()),
        )
        .sqrt();
        let drop = peak - f64::from(saddle).sqrt();
        if drop >= (f64::from(split) * peak).max(SPLIT_FLOOR) {
            return;
        }
        self.merge(left, right);
    }

    fn merge(&mut self, left: u32, right: u32) -> u32 {
        let mut left = self.find(left);
        let mut right = self.find(right);
        if left == right {
            return left;
        }
        if self.rank[left as usize] < self.rank[right as usize] {
            std::mem::swap(&mut left, &mut right);
        }
        let equal = self.rank[left as usize] == self.rank[right as usize];
        let loser = self.nodes[right as usize];
        for channel in 0..self.channels {
            let other = self.accumulators[right as usize * self.channels + channel];
            self.accumulators[left as usize * self.channels + channel].merge(&other);
        }
        self.nodes[left as usize].absorb(loser);
        if equal {
            self.rank[left as usize] += 1;
        }
        self.parent[right as usize] = left;
        left
    }

    fn fold(&mut self, node: u32) -> u32 {
        let mut root = self.find(node);
        let mut steps = 0;
        loop {
            if self.nodes[root as usize].voxels() > 0 || steps >= MAX_FOLD {
                break;
            }
            let Some(contact) = self.nodes[root as usize].contact() else {
                break;
            };
            let next = self.find(contact);
            if next == root {
                break;
            }
            root = next;
            steps += 1;
        }
        if self.nodes[root as usize].voxels() > 0 {
            root
        } else {
            EMPTY
        }
    }

    fn targets(&mut self) -> Vec<u32> {
        (0..self.topology.basins().len() as u32)
            .map(|node| self.fold(node))
            .collect()
    }

    fn check_seeds(&self, targets: &[u32]) -> Result<(), String> {
        let mut folded = vec![EMPTY; self.nodes.len()];
        for (index, node) in self.nodes.iter().enumerate() {
            let Some(seed) = node.seed() else {
                continue;
            };
            let target = targets[index];
            if target == EMPTY {
                continue;
            }
            let slot = &mut folded[target as usize];
            if *slot != EMPTY && *slot != seed {
                return Err(format!(
                    "the seed points {} and {} share one watershed basin and cannot cut it apart",
                    (*slot).min(seed) + 1,
                    (*slot).max(seed) + 1
                ));
            }
            *slot = seed;
        }
        Ok(())
    }
}

struct Meshed<'a> {
    topology: &'a SurfaceTopology,
    merged: &'a Merged<'a>,
    spec: SurfaceCut,
}

struct Chunk {
    statistics: SurfaceStatistics,
    seed: Option<u32>,
    intensities: Vec<SampleStatistics>,
    vertices: Vec<([f32; 3], [f32; 3])>,
    indices: Vec<u32>,
}

impl Meshed<'_> {
    fn chunk(&self, root: u32, triangles: &[u32]) -> Option<Chunk> {
        let info = self.topology.info();
        let patches = self.topology.patches();
        let node = self.merged.nodes[root as usize];
        let count = node.voxels() as f64;
        let volume = count * info.voxel_volume();
        if volume < f64::from(self.spec.min_volume()) {
            return None;
        }
        let channels = info.channels();
        let mean = self.merged.accumulators[root as usize * channels + info.detection()]
            .statistics()
            .mean();
        if info.span() > 0.0 && mean / info.span() < self.spec.min_mean_intensity() {
            return None;
        }
        let mut local: HashMap<[u32; 3], u32> = HashMap::with_capacity(triangles.len());
        let mut positions = Vec::with_capacity(triangles.len());
        let mut summed: Vec<[f32; 3]> = Vec::with_capacity(triangles.len());
        let mut mesh = Vec::with_capacity(triangles.len());
        for index in triangles {
            let triangle = patches.triangles()[*index as usize];
            let mut mapped = [0u32; 3];
            for (corner, vertex) in triangle.iter().enumerate() {
                let point = patches.positions()[*vertex as usize];
                let slot = local.entry(point.map(f32::to_bits)).or_insert_with(|| {
                    let slot = positions.len() as u32;
                    positions.push(point);
                    summed.push([0.0; 3]);
                    slot
                });
                mapped[corner] = *slot;
                let normal = patches.normals()[*vertex as usize];
                for (axis, value) in normal.iter().enumerate() {
                    summed[*slot as usize][axis] += value;
                }
            }
            if mapped[0] == mapped[1] || mapped[1] == mapped[2] || mapped[2] == mapped[0] {
                continue;
            }
            mesh.push(mapped);
        }
        if mesh.is_empty() {
            return None;
        }
        let components = Components::new(positions.len(), &mesh);
        let mut area = 0.0f64;
        for triangle in &mesh {
            area += triangle_area(&positions, triangle);
        }
        let open = (0..mesh.len())
            .filter(|index| !components.closed[components.slot[*index]])
            .collect::<Vec<_>>();
        let cut = cap::build(&positions, &mesh, &open);
        area += cut.area;
        if area <= 0.0 {
            return None;
        }
        let spacing = info.spacing();
        let world = info.world();
        let centroid = [0, 1, 2].map(|axis| {
            ((node.sum()[axis] / count + 0.5) * f64::from(spacing[axis])
                - 0.5 * f64::from(world[axis])) as f32
        });
        let bounds = Bounds::new(
            [0, 1, 2].map(|axis| {
                (f64::from(node.low()[axis]) * f64::from(spacing[axis])
                    - 0.5 * f64::from(world[axis])) as f32
            }),
            [0, 1, 2].map(|axis| {
                ((f64::from(node.high()[axis]) + 1.0) * f64::from(spacing[axis])
                    - 0.5 * f64::from(world[axis])) as f32
            }),
        );
        let statistics = SurfaceStatistics::new(node.voxels(), volume, area, centroid, bounds);
        if statistics.sphericity() < f64::from(self.spec.min_sphericity()) {
            return None;
        }
        let intensities = (0..channels)
            .map(|channel| {
                self.merged.accumulators[root as usize * channels + channel].statistics()
            })
            .collect();
        let mut vertices = Vec::with_capacity(positions.len());
        let mut indices = Vec::with_capacity(mesh.len() * 3 + cut.triangles.len());
        let mut welded: HashMap<u32, u32> = HashMap::with_capacity(positions.len());
        for triangle in &mesh {
            let fallback = triangle_normal(&positions, triangle);
            let mut mapped = [0u32; 3];
            for (corner, vertex) in triangle.iter().enumerate() {
                mapped[corner] = *welded.entry(*vertex).or_insert_with(|| {
                    let slot = vertices.len() as u32;
                    let normal = Vec3::from(summed[*vertex as usize]).normalize_or_zero();
                    let normal = if normal == Vec3::ZERO {
                        fallback
                    } else {
                        normal
                    };
                    vertices.push((positions[*vertex as usize], normal.to_array()));
                    slot
                });
            }
            indices.extend_from_slice(&mapped);
        }
        let base = vertices.len() as u32;
        for (position, normal) in cut.positions.iter().zip(&cut.normals) {
            vertices.push((*position, *normal));
        }
        indices.extend(cut.triangles.iter().map(|index| base + index));
        Some(Chunk {
            statistics,
            seed: node.seed(),
            intensities,
            vertices,
            indices,
        })
    }
}

struct Components {
    slot: Vec<usize>,
    closed: Vec<bool>,
}

impl Components {
    fn new(vertices: usize, triangles: &[[u32; 3]]) -> Self {
        let mut parent: Vec<u32> = (0..vertices as u32).collect();
        for triangle in triangles {
            component(&mut parent, triangle[0], triangle[1]);
            component(&mut parent, triangle[1], triangle[2]);
        }
        let mut slots: HashMap<u32, usize> = HashMap::new();
        let mut slot = Vec::with_capacity(triangles.len());
        for triangle in triangles {
            let root = component_root(&mut parent, triangle[0]);
            let count = slots.len();
            slot.push(*slots.entry(root).or_insert(count));
        }
        let mut edges: Vec<HashMap<(u32, u32), u32>> =
            (0..slots.len()).map(|_| HashMap::new()).collect();
        for (index, triangle) in triangles.iter().enumerate() {
            let counts = &mut edges[slot[index]];
            for pair in [[0usize, 1usize], [1, 2], [2, 0]] {
                *counts
                    .entry((triangle[pair[0]], triangle[pair[1]]))
                    .or_insert(0) += 1;
            }
        }
        let closed = edges
            .iter()
            .map(|counts| {
                !counts.is_empty()
                    && counts.iter().all(|((from, to), count)| {
                        *count == 1 && counts.get(&(*to, *from)) == Some(&1)
                    })
            })
            .collect();
        Self { slot, closed }
    }
}

fn triangle_area(positions: &[[f32; 3]], triangle: &[u32; 3]) -> f64 {
    let points = triangle.map(|vertex| Vec3::from(positions[vertex as usize]));
    let area = 0.5
        * (points[1] - points[0])
            .cross(points[2] - points[0])
            .length();
    if area.is_finite() && area > 0.0 {
        f64::from(area)
    } else {
        0.0
    }
}

fn triangle_normal(positions: &[[f32; 3]], triangle: &[u32; 3]) -> Vec3 {
    let points = triangle.map(|vertex| Vec3::from(positions[vertex as usize]));
    let normal = (points[1] - points[0])
        .cross(points[2] - points[0])
        .normalize_or_zero();
    if normal == Vec3::ZERO {
        Vec3::Z
    } else {
        normal
    }
}

fn component_root(parent: &mut [u32], vertex: u32) -> u32 {
    let mut root = vertex;
    while parent[root as usize] != root {
        root = parent[root as usize];
    }
    let mut current = vertex;
    while parent[current as usize] != root {
        let next = parent[current as usize];
        parent[current as usize] = root;
        current = next;
    }
    root
}

fn component(parent: &mut [u32], left: u32, right: u32) {
    let left = component_root(parent, left);
    let right = component_root(parent, right);
    if left == right {
        return;
    }
    let (low, high) = if left < right {
        (left, right)
    } else {
        (right, left)
    };
    parent[high as usize] = low;
}
