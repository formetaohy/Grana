use std::collections::{HashMap, VecDeque};

use glam::Vec3;

use super::Grid;
use super::neighbour::{HALF, OFFSET};
use super::volume;

const SEARCH_BUDGET: usize = 64;

pub(super) struct Component {
    pub members: Vec<u32>,
    pub edges: Vec<(u32, u32)>,
}

pub(super) struct Spanning {
    voxels: Vec<usize>,
    positions: Vec<[f32; 3]>,
    edges: Vec<(u32, u32)>,
    incident: Vec<Vec<u32>>,
    visited: Vec<u32>,
    generation: u32,
    parent: Vec<u32>,
    component: Vec<u32>,
}

impl Spanning {
    pub(super) fn new(mask: &[u8], grid: &Grid) -> Self {
        let voxels = mask
            .iter()
            .enumerate()
            .filter_map(|(index, value)| (*value != 0).then_some(index))
            .collect::<Vec<_>>();
        let positions = voxels
            .iter()
            .map(|index| grid.position(grid.voxel(*index)))
            .collect::<Vec<_>>();
        let count = voxels.len();
        Self {
            voxels,
            positions,
            edges: Vec::new(),
            incident: vec![Vec::new(); count],
            visited: vec![0; count],
            generation: 0,
            parent: (0..count as u32).collect(),
            component: vec![u32::MAX; count],
        }
    }

    pub(super) fn len(&self) -> usize {
        self.voxels.len()
    }

    pub(super) fn build(&mut self, distance: &[u16], grid: &Grid) {
        if self.voxels.len() < 2 {
            return;
        }
        let spacing = grid.mean_spacing();
        for steps in 1..=3usize {
            for node in 0..self.voxels.len() {
                for position in HALF {
                    let offset = OFFSET[position];
                    if offset.iter().filter(|value| **value != 0).count() != steps {
                        continue;
                    }
                    let target = (self.voxels[node] as isize
                        + offset[0] * grid.stride[0] as isize
                        + offset[1] * grid.stride[1] as isize
                        + offset[2] * grid.stride[2] as isize)
                        as usize;
                    let Ok(other) = self.voxels.binary_search(&target) else {
                        continue;
                    };
                    let length = Vec3::from(self.positions[node])
                        .distance(Vec3::from(self.positions[other]));
                    if find(&mut self.parent, node as u32) == find(&mut self.parent, other as u32) {
                        self.generation += 1;
                        if let Some((alternative, peak)) =
                            self.cycle_path(node, other, distance, spacing)
                            && length + alternative < std::f32::consts::TAU * 2.0 * peak
                        {
                            continue;
                        }
                    }
                    join(&mut self.parent, node as u32, other as u32);
                    let edge = self.edges.len() as u32;
                    self.incident[node].push(edge);
                    self.incident[other].push(edge);
                    self.edges.push((node as u32, other as u32));
                }
            }
        }
    }

    pub(super) fn components(&mut self) -> Vec<Component> {
        let mut components = Vec::<Component>::new();
        let mut roots = HashMap::new();
        for (node, voxel) in self.voxels.iter().enumerate() {
            let root = find(&mut self.parent, node as u32);
            let component = *roots.entry(root).or_insert_with(|| {
                components.push(Component {
                    members: Vec::new(),
                    edges: Vec::new(),
                });
                (components.len() - 1) as u32
            });
            self.component[node] = component;
            components[component as usize].members.push(*voxel as u32);
        }
        for (left, right) in &self.edges {
            let component = self.component[*left as usize] as usize;
            components[component].edges.push((
                self.voxels[*left as usize] as u32,
                self.voxels[*right as usize] as u32,
            ));
        }
        components
    }

    fn cycle_path(
        &mut self,
        source: usize,
        target: usize,
        distance: &[u16],
        spacing: f32,
    ) -> Option<(f32, f32)> {
        let mut queue = VecDeque::new();
        self.visited[source] = self.generation;
        queue.push_back((
            source,
            0.0f32,
            volume::radius(distance[self.voxels[source]], spacing),
        ));
        let mut explored = 0;
        while let Some((node, travelled, peak)) = queue.pop_front() {
            explored += 1;
            if explored > SEARCH_BUDGET {
                return None;
            }
            for edge in &self.incident[node] {
                let (left, right) = self.edges[*edge as usize];
                let next = if left as usize == node {
                    right as usize
                } else {
                    left as usize
                };
                if self.visited[next] == self.generation {
                    continue;
                }
                let step =
                    Vec3::from(self.positions[node]).distance(Vec3::from(self.positions[next]));
                let travelled = travelled + step;
                let peak = peak.max(volume::radius(distance[self.voxels[next]], spacing));
                if next == target {
                    return Some((travelled, peak));
                }
                self.visited[next] = self.generation;
                queue.push_back((next, travelled, peak));
            }
        }
        None
    }
}

fn find(parent: &mut [u32], node: u32) -> u32 {
    let mut root = node;
    while parent[root as usize] != root {
        root = parent[root as usize];
    }
    let mut current = node;
    while parent[current as usize] != root {
        let next = parent[current as usize];
        parent[current as usize] = root;
        current = next;
    }
    root
}

fn join(parent: &mut [u32], left: u32, right: u32) {
    let left = find(parent, left);
    let right = find(parent, right);
    if left != right {
        parent[right as usize] = left;
    }
}
