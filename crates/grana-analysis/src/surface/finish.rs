use std::collections::HashMap;

use glam::Vec3;
use grana_geometry::{Bounds, Mesh, MeshVertex};
use grana_object::{IntensityTable, SurfaceSet, SurfaceStatistics};

use super::assemble::Assembler;
use super::{EMPTY, Node, find, union};

const MAX_FOLD: u32 = 8;

struct Shell {
    target: u32,
    volume: f64,
    moment: [f64; 3],
    low: [f32; 3],
    high: [f32; 3],
    dropped: bool,
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
        let gap = f64::from(self.plan.merge_gap);
        let channels = self.plan.channels;
        for ((left, right), neck) in seams {
            let left = find(&mut self.nodes, left);
            let right = find(&mut self.nodes, right);
            if left == right {
                continue;
            }
            let reach = f64::from(self.nodes[left as usize].reach)
                .sqrt()
                .min(f64::from(self.nodes[right as usize].reach).sqrt());
            let drop = reach - f64::from(neck).sqrt();
            if drop < gap {
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

    pub(super) fn build(mut self) -> SurfaceSet {
        let plan = self.plan;
        let mut targets = vec![EMPTY; self.nodes.len()];
        for (index, target) in targets.iter_mut().enumerate() {
            *target = fold(&mut self.nodes, index as u32);
        }
        let candidates = (0..self.nodes.len())
            .filter(|root| {
                find(&mut self.nodes, *root as u32) == *root as u32 && self.nodes[*root].voxels > 0
            })
            .map(|root| root as u32)
            .collect::<Vec<_>>();
        let mut weld: HashMap<u64, u32> = HashMap::new();
        let mut welded = Vec::with_capacity(self.vertices.len());
        let mut positions = Vec::with_capacity(self.vertices.len());
        let mut normals: Vec<Vec3> = Vec::with_capacity(self.vertices.len());
        let mut first_normals: Vec<[f32; 3]> = Vec::with_capacity(self.vertices.len());
        for (index, vertex) in self.vertices.iter().enumerate() {
            let key = self.keys[index];
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
        let mut triangles = Vec::with_capacity(self.indices.len() / 3);
        for triangle in self.indices.as_chunks::<3>().0 {
            triangles.push(triangle.map(|index| welded[index as usize]));
        }
        let mut triangle_target = Vec::with_capacity(triangles.len());
        for object in &self.triangle_objects {
            triangle_target.push(targets[*object as usize]);
        }
        let mut parent: Vec<u32> = (0..positions.len() as u32).collect();
        for triangle in &triangles {
            component(&mut parent, triangle[0], triangle[1]);
            component(&mut parent, triangle[1], triangle[2]);
        }
        let mut shells: HashMap<u32, usize> = HashMap::new();
        let mut shell_of = Vec::with_capacity(triangles.len());
        for triangle in &triangles {
            let root = component_root(&mut parent, triangle[0]);
            let count = shells.len();
            let slot = *shells.entry(root).or_insert(count);
            shell_of.push(slot);
        }
        let mut parts = (0..shells.len())
            .map(|_| Shell {
                target: EMPTY,
                volume: 0.0,
                moment: [0.0; 3],
                low: [f32::INFINITY; 3],
                high: [f32::NEG_INFINITY; 3],
                dropped: false,
            })
            .collect::<Vec<_>>();
        for (index, triangle) in triangles.iter().enumerate() {
            let slot = shell_of[index];
            let points = triangle.map(|vertex| Vec3::from(positions[vertex as usize]));
            let volume = points[0].dot(points[1].cross(points[2])) / 6.0;
            let centroid = (points[0] + points[1] + points[2]) / 4.0;
            let part = &mut parts[slot];
            if part.target == EMPTY {
                part.target = triangle_target[index];
            }
            part.volume += f64::from(volume);
            for axis in 0..3 {
                part.moment[axis] += f64::from(volume) * f64::from(centroid[axis]);
            }
            for point in points {
                for axis in 0..3 {
                    part.low[axis] = part.low[axis].min(point[axis]);
                    part.high[axis] = part.high[axis].max(point[axis]);
                }
            }
        }
        let epsilon = 0.5 * plan.voxel_volume;
        let mut edge_indices = vec![usize::MAX; parts.len()];
        let mut edges: Vec<HashMap<(u32, u32), u32>> = Vec::new();
        for (slot, part) in parts.iter().enumerate() {
            if part.volume < -epsilon {
                edge_indices[slot] = edges.len();
                edges.push(HashMap::new());
            }
        }
        for (index, triangle) in triangles.iter().enumerate() {
            let edge_index = edge_indices[shell_of[index]];
            if edge_index == usize::MAX {
                continue;
            }
            for pair in [[0usize, 1usize], [1, 2], [2, 0]] {
                let first = triangle[pair[0]];
                let second = triangle[pair[1]];
                let key = (first.min(second), first.max(second));
                *edges[edge_index].entry(key).or_insert(0u32) += 1;
            }
        }
        for (slot, edge_index) in edge_indices.iter().enumerate() {
            if *edge_index != usize::MAX && edges[*edge_index].values().all(|count| *count == 2) {
                parts[slot].dropped = true;
            }
        }
        for part in parts.iter_mut() {
            if part.target != EMPTY {
                continue;
            }
            let centre = Vec3::from([0, 1, 2].map(|axis| 0.5 * (part.low[axis] + part.high[axis])));
            part.target = nearest(&self.nodes, &candidates, centre);
        }
        let mut areas = vec![0.0f64; self.nodes.len()];
        for (index, triangle) in triangles.iter().enumerate() {
            if parts[shell_of[index]].dropped {
                continue;
            }
            let target = triangle_target[index];
            if target == EMPTY {
                continue;
            }
            let points = triangle.map(|vertex| Vec3::from(positions[vertex as usize]));
            let area = 0.5
                * (points[1] - points[0])
                    .cross(points[2] - points[0])
                    .length();
            if area.is_finite() && area > 0.0 {
                areas[target as usize] += f64::from(area);
            }
        }
        let mut cavity_volume = vec![0.0f64; self.nodes.len()];
        let mut cavity_moment = vec![[0.0f64; 3]; self.nodes.len()];
        let mut cavity_low = vec![[u32::MAX; 3]; self.nodes.len()];
        let mut cavity_high = vec![[0u32; 3]; self.nodes.len()];
        let mut cavity_voxels = vec![0u64; self.nodes.len()];
        for part in &parts {
            if !part.dropped || part.target == EMPTY {
                continue;
            }
            let volume = -part.volume;
            if !volume.is_finite() || volume <= epsilon {
                continue;
            }
            let target = part.target as usize;
            let voxels = (volume / plan.voxel_volume).round().max(1.0) as u64;
            cavity_voxels[target] += voxels;
            cavity_volume[target] += volume;
            for (slot, moment) in cavity_moment[target].iter_mut().zip(&part.moment) {
                *slot -= *moment;
            }
            for axis in 0..3 {
                let low =
                    ((part.low[axis] + 0.5 * plan.world[axis]) / plan.spacing[axis] - 0.5).floor();
                let high =
                    ((part.high[axis] + 0.5 * plan.world[axis]) / plan.spacing[axis] - 0.5).ceil();
                cavity_low[target][axis] = cavity_low[target][axis]
                    .min(low.clamp(0.0, plan.extent[axis] as f32 - 1.0) as u32);
                cavity_high[target][axis] = cavity_high[target][axis]
                    .max(high.clamp(0.0, plan.extent[axis] as f32 - 1.0) as u32);
            }
        }
        let mut objects = Vec::new();
        let mut intensities = Vec::new();
        let mut map = vec![EMPTY; self.nodes.len()];
        for root in 0..self.nodes.len() {
            if find(&mut self.nodes, root as u32) != root as u32 {
                continue;
            }
            let node = self.nodes[root];
            if node.voxels == 0 {
                continue;
            }
            let voxels = node.voxels + cavity_voxels[root];
            let volume = cavity_volume[root] + node.voxels as f64 * plan.voxel_volume;
            let low = [0, 1, 2].map(|axis| {
                if cavity_voxels[root] > 0 {
                    cavity_low[root][axis].min(node.low[axis])
                } else {
                    node.low[axis]
                }
            });
            let high = [0, 1, 2].map(|axis| {
                if cavity_voxels[root] > 0 {
                    cavity_high[root][axis].max(node.high[axis])
                } else {
                    node.high[axis]
                }
            });
            let mut sum = node.sum;
            if cavity_voxels[root] > 0 {
                let centroid =
                    [0, 1, 2].map(|axis| cavity_moment[root][axis] / -cavity_volume[root]);
                for axis in 0..3 {
                    sum[axis] += centroid[axis] * cavity_voxels[root] as f64;
                }
            }
            let count = voxels as f64;
            let centroid = [0, 1, 2].map(|axis| {
                ((sum[axis] / count + 0.5) * f64::from(plan.spacing[axis])
                    - 0.5 * f64::from(plan.world[axis])) as f32
            });
            let bounds = Bounds::new(
                [0, 1, 2].map(|axis| {
                    (f64::from(low[axis]) * f64::from(plan.spacing[axis])
                        - 0.5 * f64::from(plan.world[axis])) as f32
                }),
                [0, 1, 2].map(|axis| {
                    ((f64::from(high[axis]) + 1.0) * f64::from(plan.spacing[axis])
                        - 0.5 * f64::from(plan.world[axis])) as f32
                }),
            );
            let area = areas[root];
            if volume < f64::from(plan.params.min_volume()) || area <= 0.0 {
                continue;
            }
            let statistics = SurfaceStatistics::new(voxels, volume, area, centroid, bounds);
            if statistics.sphericity() < f64::from(plan.params.min_sphericity()) {
                continue;
            }
            let mean = self.accumulators[root * plan.channels + plan.detection]
                .statistics()
                .mean();
            if plan.span > 0.0 && mean / plan.span < plan.params.min_mean_intensity() {
                continue;
            }
            map[root] = objects.len() as u32;
            objects.push(statistics);
            for channel in 0..plan.channels {
                intensities.push(self.accumulators[root * plan.channels + channel].statistics());
            }
        }
        let mut vertex_map: HashMap<(u32, u32), u32> = HashMap::new();
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        for (index, triangle) in triangles.iter().enumerate() {
            if parts[shell_of[index]].dropped {
                continue;
            }
            let mut object = triangle_target[index];
            if object == EMPTY {
                let points = triangle.map(|vertex| Vec3::from(positions[vertex as usize]));
                let centre = (points[0] + points[1] + points[2]) / 3.0;
                object = nearest(&self.nodes, &candidates, centre);
            } else {
                object = map[object as usize];
            }
            if object == EMPTY {
                continue;
            }
            let mut mapped = [0u32; 3];
            for (slot, vertex) in triangle.iter().enumerate() {
                let vertex = *vertex;
                let key = (vertex, object);
                let mapped_vertex = match vertex_map.get(&key) {
                    Some(mapped_vertex) => *mapped_vertex,
                    None => {
                        let welded = vertex as usize;
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
                mapped[slot] = mapped_vertex;
            }
            indices.extend_from_slice(&mapped);
        }
        SurfaceSet::new(
            plan.level_index,
            Mesh::new(vertices, indices),
            objects,
            IntensityTable::new(plan.channels, intensities),
        )
    }
}

fn nearest(nodes: &[Node], candidates: &[u32], point: Vec3) -> u32 {
    candidates
        .iter()
        .copied()
        .min_by(|left, right| {
            distance(nodes, *left, point)
                .total_cmp(&distance(nodes, *right, point))
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
