use std::collections::HashMap;

use glam::Vec3;
use grana_geometry::{Bounds, Mesh, MeshVertex};
use grana_object::{IntensityTable, SurfaceSet, SurfaceStatistics};

use super::assemble::Assembler;
use super::{EMPTY, Node, SPLIT_FLOOR, cap, find, union};

const MAX_FOLD: u32 = 8;

struct Component {
    volume: f64,
    closed: bool,
}

impl Assembler<'_> {
    pub(super) fn merge_basins(&mut self) {
        let mut seams: HashMap<(u32, u32), u32> = HashMap::new();
        for (left, right, neck) in std::mem::take(&mut self.necks) {
            let left = find(&mut self.nodes, left);
            let right = find(&mut self.nodes, right);
            if left == right {
                continue;
            }
            let key = (left.min(right), left.max(right));
            let slot = seams.entry(key).or_insert(0);
            *slot = (*slot).max(neck);
        }
        let mut seams = seams.into_iter().collect::<Vec<_>>();
        seams.sort_unstable_by(|(left_key, left_neck), (right_key, right_neck)| {
            right_neck.cmp(left_neck).then(left_key.cmp(right_key))
        });
        let split = self.plan.split;
        let channels = self.plan.channels;
        for ((left, right), neck) in seams {
            let left = find(&mut self.nodes, left);
            let right = find(&mut self.nodes, right);
            if left == right {
                continue;
            }
            let seeds = (
                self.nodes[left as usize].seed,
                self.nodes[right as usize].seed,
            );
            if seeds.0 != EMPTY && seeds.1 != EMPTY && seeds.0 != seeds.1 {
                continue;
            }
            let peak = f64::from(
                self.nodes[left as usize]
                    .reach
                    .min(self.nodes[right as usize].reach),
            )
            .sqrt();
            let drop = peak - f64::from(neck).sqrt();
            if drop < (split * peak).max(SPLIT_FLOOR) {
                union(
                    &mut self.nodes,
                    &mut self.accumulators,
                    channels,
                    left,
                    right,
                );
            }
        }
    }

    pub(super) fn build(self) -> Result<SurfaceSet, String> {
        let plan = self.plan;
        let mut nodes = self.nodes;
        let accumulators = self.accumulators;
        let raw_vertices = self.vertices;
        let keys = self.keys;
        let raw_indices = self.indices;
        let objects_of_triangle = self.triangle_objects;
        let mut targets = vec![EMPTY; nodes.len()];
        for (index, target) in targets.iter_mut().enumerate() {
            *target = fold(&mut nodes, index as u32);
        }
        let mut folded = vec![EMPTY; nodes.len()];
        for (index, node) in nodes.iter().enumerate() {
            if node.seed == EMPTY {
                continue;
            }
            let target = targets[index];
            if target == EMPTY {
                continue;
            }
            let slot = &mut folded[target as usize];
            if *slot != EMPTY && *slot != node.seed {
                return Err(format!(
                    "the seed points {} and {} share one watershed basin and cannot cut it apart",
                    (*slot).min(node.seed) + 1,
                    (*slot).max(node.seed) + 1
                ));
            }
            *slot = node.seed;
        }
        let candidates = (0..nodes.len())
            .filter(|root| {
                find(&mut nodes, *root as u32) == *root as u32 && nodes[*root].voxels > 0
            })
            .map(|root| root as u32)
            .collect::<Vec<_>>();
        let mut weld: HashMap<u64, u32> = HashMap::new();
        let mut welded = Vec::with_capacity(raw_vertices.len());
        let mut positions = Vec::with_capacity(raw_vertices.len());
        let mut normals: Vec<Vec3> = Vec::with_capacity(raw_vertices.len());
        let mut first_normals: Vec<[f32; 3]> = Vec::with_capacity(raw_vertices.len());
        for (index, vertex) in raw_vertices.iter().enumerate() {
            let key = keys[index];
            let slot = match weld.get(&key) {
                Some(slot) => *slot,
                None => {
                    let slot = positions.len() as u32;
                    weld.insert(key, slot);
                    positions.push(vertex.position());
                    normals.push(Vec3::ZERO);
                    first_normals.push(vertex.normal());
                    slot
                }
            };
            normals[slot as usize] += Vec3::from(vertex.normal());
            welded.push(slot);
        }
        let mut triangles = Vec::with_capacity(raw_indices.len() / 3);
        for triangle in raw_indices.as_chunks::<3>().0 {
            triangles.push(triangle.map(|index| welded[index as usize]));
        }
        let mut triangle_object = Vec::with_capacity(triangles.len());
        for (index, object) in objects_of_triangle.iter().enumerate() {
            triangle_object.push(if *object == EMPTY {
                nearest_triangle(&nodes, &candidates, &positions, &triangles[index])
            } else {
                targets[*object as usize]
            });
        }
        let mut parent: Vec<u32> = (0..positions.len() as u32).collect();
        for triangle in &triangles {
            component(&mut parent, triangle[0], triangle[1]);
            component(&mut parent, triangle[1], triangle[2]);
        }
        let mut slots: HashMap<u32, usize> = HashMap::new();
        let mut component_of = Vec::with_capacity(triangles.len());
        for triangle in &triangles {
            let root = component_root(&mut parent, triangle[0]);
            let count = slots.len();
            let slot = *slots.entry(root).or_insert(count);
            component_of.push(slot);
        }
        let mut components = (0..slots.len())
            .map(|_| Component {
                volume: 0.0,
                closed: false,
            })
            .collect::<Vec<_>>();
        for (index, triangle) in triangles.iter().enumerate() {
            let points = triangle.map(|vertex| Vec3::from(positions[vertex as usize]));
            let volume = points[0].dot(points[1].cross(points[2])) / 6.0;
            components[component_of[index]].volume += f64::from(volume);
        }
        for component in components.iter_mut() {
            component.closed = component.volume < 0.0;
        }
        let mut edges = vec![HashMap::<(u32, u32), u32>::new(); components.len()];
        for (index, triangle) in triangles.iter().enumerate() {
            if !components[component_of[index]].closed {
                continue;
            }
            for pair in [[0usize, 1usize], [1, 2], [2, 0]] {
                let first = triangle[pair[0]];
                let second = triangle[pair[1]];
                let key = (first.min(second), first.max(second));
                *edges[component_of[index]].entry(key).or_insert(0u32) += 1;
            }
        }
        for (index, component) in components.iter_mut().enumerate() {
            component.closed = component.closed
                && !edges[index].is_empty()
                && edges[index].values().all(|count| *count == 2);
        }
        let mut areas = vec![0.0f64; nodes.len()];
        for (index, triangle) in triangles.iter().enumerate() {
            let object = triangle_object[index];
            if object == EMPTY {
                continue;
            }
            let points = triangle.map(|vertex| Vec3::from(positions[vertex as usize]));
            let area = 0.5
                * (points[1] - points[0])
                    .cross(points[2] - points[0])
                    .length();
            if area.is_finite() && area > 0.0 {
                areas[object as usize] += f64::from(area);
            }
        }
        let mut surface: HashMap<u32, Vec<usize>> = HashMap::new();
        for index in 0..triangles.len() {
            if components[component_of[index]].closed {
                continue;
            }
            let object = triangle_object[index];
            if object != EMPTY {
                surface.entry(object).or_default().push(index);
            }
        }
        let mut objects = Vec::new();
        let mut intensities = Vec::new();
        let mut kept = Vec::new();
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        let mut vertex_map: HashMap<(u32, u32), u32> = HashMap::new();
        for root in 0..nodes.len() {
            if find(&mut nodes, root as u32) != root as u32 {
                continue;
            }
            let node = nodes[root];
            if node.voxels == 0 {
                continue;
            }
            let Some(selected) = surface.get(&(root as u32)) else {
                continue;
            };
            let cut = cap::build(&positions, &triangles, selected);
            let area = areas[root] + cut.area;
            let count = node.voxels as f64;
            let centroid = [0, 1, 2].map(|axis| {
                ((node.sum[axis] / count + 0.5) * f64::from(plan.spacing[axis])
                    - 0.5 * f64::from(plan.world[axis])) as f32
            });
            let bounds = Bounds::new(
                [0, 1, 2].map(|axis| {
                    (f64::from(node.low[axis]) * f64::from(plan.spacing[axis])
                        - 0.5 * f64::from(plan.world[axis])) as f32
                }),
                [0, 1, 2].map(|axis| {
                    ((f64::from(node.high[axis]) + 1.0) * f64::from(plan.spacing[axis])
                        - 0.5 * f64::from(plan.world[axis])) as f32
                }),
            );
            let volume = count * plan.voxel_volume;
            if volume < f64::from(plan.params.min_volume()) || area <= 0.0 {
                continue;
            }
            let statistics = SurfaceStatistics::new(node.voxels, volume, area, centroid, bounds);
            if statistics.sphericity() < f64::from(plan.params.min_sphericity()) {
                continue;
            }
            let mean = accumulators[root * plan.channels + plan.detection]
                .statistics()
                .mean();
            if plan.span > 0.0 && mean / plan.span < plan.params.min_mean_intensity() {
                continue;
            }
            let object = objects.len() as u32;
            objects.push(statistics);
            if folded[root] != EMPTY {
                kept.push(folded[root]);
            }
            for channel in 0..plan.channels {
                intensities.push(accumulators[root * plan.channels + channel].statistics());
            }
            for index in selected {
                let triangle = triangles[*index];
                let mut mapped = [0u32; 3];
                for (slot, vertex) in triangle.iter().enumerate() {
                    let key = (*vertex, object);
                    mapped[slot] = match vertex_map.get(&key) {
                        Some(mapped_vertex) => *mapped_vertex,
                        None => {
                            let welded = *vertex as usize;
                            let normal = normals[welded].normalize_or_zero();
                            let normal = if normal == Vec3::ZERO {
                                Vec3::from(first_normals[welded])
                            } else {
                                normal
                            };
                            let mapped_vertex = vertices.len() as u32;
                            vertices.push(MeshVertex::new(
                                positions[welded],
                                normal.to_array(),
                                object,
                            ));
                            vertex_map.insert(key, mapped_vertex);
                            mapped_vertex
                        }
                    };
                }
                indices.extend_from_slice(&mapped);
            }
            let base = vertices.len() as u32;
            for (position, normal) in cut.positions.iter().zip(&cut.normals) {
                vertices.push(MeshVertex::new(*position, *normal, object));
            }
            for index in &cut.triangles {
                indices.push(base + *index);
            }
        }
        if kept.len() != self.seed_count {
            let missing = (0..self.seed_count)
                .find(|seed| !kept.contains(&(*seed as u32)))
                .expect("the kept seeds cover fewer seed points");
            return Err(format!(
                "the seed point {} no longer marks an object of the detected surface",
                missing + 1
            ));
        }
        Ok(SurfaceSet::new(
            plan.level_index,
            Mesh::new(vertices, indices),
            objects,
            IntensityTable::new(plan.channels, intensities),
        ))
    }
}

fn nearest_triangle(
    nodes: &[Node],
    candidates: &[u32],
    positions: &[[f32; 3]],
    triangle: &[u32; 3],
) -> u32 {
    let points = triangle.map(|vertex| Vec3::from(positions[vertex as usize]));
    let centre = (points[0] + points[1] + points[2]) / 3.0;
    candidates
        .iter()
        .copied()
        .min_by(|left, right| {
            distance(nodes, *left, centre)
                .total_cmp(&distance(nodes, *right, centre))
                .then(left.cmp(right))
        })
        .unwrap_or(EMPTY)
}

fn distance(nodes: &[Node], root: u32, point: Vec3) -> f32 {
    let node = nodes[root as usize];
    let mut total = 0.0f32;
    for axis in 0..3 {
        let low = node.low[axis] as f32;
        let high = (node.high[axis] + 1) as f32;
        let value = point[axis];
        total += (low - value).max(value - high).max(0.0).powi(2);
    }
    total
}

fn fold(nodes: &mut [Node], node: u32) -> u32 {
    let mut root = find(nodes, node);
    let mut steps = 0;
    while nodes[root as usize].voxels == 0
        && nodes[root as usize].contact != EMPTY
        && steps < MAX_FOLD
    {
        let next = find(nodes, nodes[root as usize].contact);
        if next == root {
            break;
        }
        root = next;
        steps += 1;
    }
    if nodes[root as usize].voxels == 0 {
        EMPTY
    } else {
        root
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
