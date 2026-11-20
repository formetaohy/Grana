use std::collections::HashMap;

use glam::Vec3;
use grana_geometry::{Ball, Bounds, Union};
use grana_object::{FilamentStatistics, FilamentTopology};

use super::Grid;
use super::spanning::Spanning;

pub(super) struct Point {
    pub position: [f32; 3],
    pub radius: f32,
}

pub(super) struct Filament {
    pub nodes: Vec<usize>,
    pub balls: Vec<Ball>,
    pub statistics: FilamentStatistics,
}

pub(super) fn trace(mask: &[u8], distance: &[u16], grid: &Grid, min_length: f32) -> Vec<Filament> {
    let mut spanning = Spanning::new(mask, grid);
    if spanning.len() < 2 {
        return Vec::new();
    }
    spanning.build(distance, grid);
    let radius = |index: usize| grid.radius(distance[index]);
    let components = spanning.components();
    let mut filaments = components
        .iter()
        .filter_map(|component| {
            let mut graph = Graph::new(&component.members, &component.edges, grid);
            graph.filament(grid, &radius, min_length)
        })
        .collect::<Vec<_>>();
    filaments.sort_by(|left, right| {
        right
            .statistics
            .length()
            .total_cmp(&left.statistics.length())
    });
    filaments
}

pub(super) fn mesh(
    filament: &Filament,
    object: u32,
) -> Option<(Vec<grana_geometry::MeshVertex>, Vec<u32>)> {
    let union = Union::new(filament.balls.clone())?;
    Some(union.mesh(object))
}

struct Node {
    voxels: Vec<usize>,
    position: [f32; 3],
    alive: bool,
}

struct Graph {
    nodes: Vec<Node>,
    edges: Vec<(u32, u32, f32)>,
    used: Vec<bool>,
    incident: Vec<Vec<u32>>,
}

impl Graph {
    fn new(members: &[u32], edges: &[(u32, u32)], grid: &Grid) -> Self {
        let local = members
            .iter()
            .enumerate()
            .map(|(index, node)| (*node, index as u32))
            .collect::<HashMap<_, _>>();
        let nodes = members
            .iter()
            .map(|voxel| Node {
                voxels: vec![*voxel as usize],
                position: grid.position(grid.voxel(*voxel as usize)),
                alive: true,
            })
            .collect::<Vec<_>>();
        let mut incident = vec![Vec::new(); members.len()];
        let mut converted = Vec::with_capacity(edges.len());
        for (left, right) in edges {
            let left = local[left];
            let right = local[right];
            let length = Vec3::from(nodes[left as usize].position)
                .distance(Vec3::from(nodes[right as usize].position));
            let edge = converted.len() as u32;
            incident[left as usize].push(edge);
            incident[right as usize].push(edge);
            converted.push((left, right, length));
        }
        Self {
            nodes,
            edges: converted,
            used: vec![true; edges.len()],
            incident,
        }
    }

    fn degree(&self, node: u32) -> usize {
        self.incident[node as usize]
            .iter()
            .filter(|edge| self.used[**edge as usize])
            .count()
    }

    fn other(&self, edge: u32, node: u32) -> u32 {
        let (left, right, _) = self.edges[edge as usize];
        if left == node { right } else { left }
    }

    fn next(&self, node: u32, previous: u32) -> Option<u32> {
        self.incident[node as usize]
            .iter()
            .copied()
            .find(|edge| *edge != previous && self.used[*edge as usize])
    }

    fn merge_junctions(&mut self) {
        loop {
            let contract = (0..self.edges.len())
                .map(|edge| {
                    self.used[edge]
                        && self.degree(self.edges[edge].0) >= 3
                        && self.degree(self.edges[edge].1) >= 3
                })
                .collect::<Vec<_>>();
            if !contract.iter().any(|value| *value) {
                break;
            }
            self.contract(&contract);
        }
    }

    fn contract(&mut self, contract: &[bool]) {
        let count = self.nodes.len() as u32;
        let mut parent = (0..count).collect::<Vec<_>>();
        let mut merged = false;
        for (edge, selected) in contract.iter().enumerate() {
            if *selected && self.used[edge] {
                let (left, right, _) = self.edges[edge];
                join(&mut parent, left, right);
                merged = true;
            }
        }
        if !merged {
            return;
        }
        let mut cluster_of = vec![u32::MAX; count as usize];
        let mut nodes = Vec::<Node>::new();
        for node in 0..count {
            let root = find(&mut parent, node);
            let cluster = if cluster_of[root as usize] == u32::MAX {
                let cluster = nodes.len() as u32;
                cluster_of[root as usize] = cluster;
                nodes.push(Node {
                    voxels: Vec::new(),
                    position: [0.0; 3],
                    alive: false,
                });
                cluster
            } else {
                cluster_of[root as usize]
            };
            let target = &mut nodes[cluster as usize];
            target
                .voxels
                .extend_from_slice(&self.nodes[node as usize].voxels);
            target.alive |= self.nodes[node as usize].alive;
            for axis in 0..3 {
                target.position[axis] += self.nodes[node as usize].position[axis];
            }
        }
        for node in &mut nodes {
            let count = node.voxels.len() as f32;
            for value in &mut node.position {
                *value /= count;
            }
        }
        let mut edges = Vec::new();
        let mut incident = vec![Vec::new(); nodes.len()];
        let mut seen = HashMap::new();
        for edge in 0..self.edges.len() {
            if !self.used[edge] {
                continue;
            }
            let (left, right, _) = self.edges[edge];
            let left = cluster_of[find(&mut parent, left) as usize];
            let right = cluster_of[find(&mut parent, right) as usize];
            if left == right {
                continue;
            }
            let key = (left.min(right), left.max(right));
            if seen.insert(key, ()).is_some() {
                continue;
            }
            let length = Vec3::from(nodes[left as usize].position)
                .distance(Vec3::from(nodes[right as usize].position));
            let edge = edges.len() as u32;
            incident[left as usize].push(edge);
            incident[right as usize].push(edge);
            edges.push((left, right, length));
        }
        self.nodes = nodes;
        self.used = vec![true; edges.len()];
        self.edges = edges;
        self.incident = incident;
    }

    fn drop_orphans(&mut self) -> bool {
        let mut dropped = false;
        for node in 0..self.nodes.len() as u32 {
            if self.nodes[node as usize].alive && self.degree(node) == 0 {
                self.nodes[node as usize].alive = false;
                dropped = true;
            }
        }
        dropped
    }

    fn prune(&mut self, min_length: f32) -> bool {
        let mut changed = false;
        loop {
            let leaves = (0..self.nodes.len() as u32)
                .filter(|node| self.nodes[*node as usize].alive && self.degree(*node) == 1)
                .collect::<Vec<_>>();
            let mut removed = false;
            for leaf in leaves {
                if !self.nodes[leaf as usize].alive || self.degree(leaf) != 1 {
                    continue;
                }
                let mut trail = Vec::new();
                let mut path = vec![leaf];
                let mut length = 0.0;
                let mut current = leaf;
                let mut previous = u32::MAX;
                let mut terminal = None;
                while let Some(edge) = self.next(current, previous) {
                    length += self.edges[edge as usize].2;
                    trail.push(edge);
                    let next = self.other(edge, current);
                    path.push(next);
                    if self.degree(next) != 2 {
                        terminal = Some(next);
                        break;
                    }
                    previous = edge;
                    current = next;
                }
                let Some(terminal) = terminal else {
                    continue;
                };
                let detached = self.degree(terminal) == 1;
                if length >= min_length || terminal == leaf {
                    continue;
                }
                for edge in trail {
                    self.used[edge as usize] = false;
                }
                for node in path {
                    if !detached && node == terminal {
                        continue;
                    }
                    self.nodes[node as usize].alive = false;
                }
                removed = true;
            }
            removed |= self.drop_orphans();
            changed |= removed;
            if !removed {
                break;
            }
        }
        changed
    }

    fn segments(&self) -> Vec<Vec<u32>> {
        let count = self.nodes.len();
        let critical = (0..count as u32)
            .map(|node| self.nodes[node as usize].alive && self.degree(node) != 2)
            .collect::<Vec<_>>();
        let mut used = vec![false; self.edges.len()];
        let mut paths = Vec::new();
        if !critical.iter().any(|value| *value) {
            let Some(start) = (0..count as u32).find(|node| self.nodes[*node as usize].alive)
            else {
                return paths;
            };
            let mut path = vec![start];
            let mut current = start;
            let mut previous = u32::MAX;
            while let Some(edge) = self.next(current, previous) {
                if used[edge as usize] {
                    break;
                }
                used[edge as usize] = true;
                let next = self.other(edge, current);
                path.push(next);
                if next == start {
                    break;
                }
                previous = edge;
                current = next;
            }
            paths.push(path);
            return paths;
        }
        for node in 0..count as u32 {
            if !critical[node as usize] {
                continue;
            }
            for edge in self.incident[node as usize].clone() {
                if used[edge as usize] || !self.used[edge as usize] {
                    continue;
                }
                used[edge as usize] = true;
                let mut path = vec![node];
                let mut previous = edge;
                let mut target = self.other(edge, node);
                path.push(target);
                while !critical[target as usize] {
                    let Some(next) = self.next(target, previous) else {
                        break;
                    };
                    used[next as usize] = true;
                    previous = next;
                    target = self.other(next, target);
                    path.push(target);
                }
                paths.push(path);
            }
        }
        paths
    }

    fn point(&self, node: u32, radius: &impl Fn(usize) -> f32) -> Point {
        Point {
            position: self.nodes[node as usize].position,
            radius: self.nodes[node as usize]
                .voxels
                .iter()
                .map(|voxel| radius(*voxel))
                .fold(0.0f32, f32::max),
        }
    }

    fn filament(
        &mut self,
        grid: &Grid,
        radius: &impl Fn(usize) -> f32,
        min_length: f32,
    ) -> Option<Filament> {
        self.merge_junctions();
        self.prune(min_length);
        let paths = self.segments();
        if paths.is_empty() {
            return None;
        }
        let mut shaped = Vec::with_capacity(paths.len());
        for path in &paths {
            let mut points = path
                .iter()
                .map(|node| self.point(*node, radius))
                .collect::<Vec<_>>();
            smooth(&mut points);
            shaped.push(points);
        }
        let balls = body(&shaped);
        if balls.is_empty() {
            return None;
        }
        let mut alive = Vec::new();
        let mut low = [f32::INFINITY; 3];
        let mut high = [f32::NEG_INFINITY; 3];
        let mut sum = [0.0f64; 3];
        let mut terminals = 0;
        let mut junctions = 0;
        let mut voxels = 0u64;
        for node in 0..self.nodes.len() as u32 {
            if !self.nodes[node as usize].alive {
                continue;
            }
            for voxel in &self.nodes[node as usize].voxels {
                alive.push(*voxel);
                let position = grid.position(grid.voxel(*voxel));
                for axis in 0..3 {
                    low[axis] = low[axis].min(position[axis]);
                    high[axis] = high[axis].max(position[axis]);
                    sum[axis] += f64::from(position[axis]);
                }
                voxels += 1;
            }
            match self.degree(node) {
                1 => terminals += 1,
                degree if degree >= 3 => junctions += 1,
                _ => {}
            }
        }
        let centroid = [0, 1, 2].map(|axis| (sum[axis] / voxels as f64) as f32);
        let statistics = FilamentStatistics::new(
            length(&shaped),
            diameter(&shaped),
            Union::new(balls.clone()).map_or(0.0, |union| union.volume()),
            FilamentTopology::new(
                shaped.len() as u32,
                terminals,
                junctions,
                alive.len() as u32,
            ),
            centroid,
            Bounds::new(low, high),
        );
        Some(Filament {
            nodes: alive,
            balls,
            statistics,
        })
    }
}

fn body(paths: &[Vec<Point>]) -> Vec<Ball> {
    let mut balls = Vec::new();
    for path in paths {
        for pair in path.windows(2) {
            balls.push(Ball::new(pair[0].position, pair[0].radius));
            let distance = Vec3::from(pair[1].position).distance(Vec3::from(pair[0].position));
            let step = pair[0].radius.min(pair[1].radius);
            let count = (distance / step).ceil() as usize;
            for step in 1..count {
                let fraction = step as f32 / count as f32;
                let center = [0, 1, 2].map(|axis| {
                    pair[0].position[axis]
                        + (pair[1].position[axis] - pair[0].position[axis]) * fraction
                });
                balls.push(Ball::new(
                    center,
                    pair[0].radius + (pair[1].radius - pair[0].radius) * fraction,
                ));
            }
        }
        if let Some(last) = path.last() {
            balls.push(Ball::new(last.position, last.radius));
        }
    }
    balls
}

fn length(paths: &[Vec<Point>]) -> f64 {
    paths
        .iter()
        .flat_map(|path| path.windows(2))
        .map(|pair| f64::from(Vec3::from(pair[1].position).distance(Vec3::from(pair[0].position))))
        .sum()
}

fn diameter(paths: &[Vec<Point>]) -> f64 {
    let mut length = 0.0f64;
    let mut weighted = 0.0f64;
    for path in paths {
        for pair in path.windows(2) {
            let distance =
                f64::from(Vec3::from(pair[1].position).distance(Vec3::from(pair[0].position)));
            if distance > 0.0 {
                length += distance;
                weighted += distance * f64::from(pair[0].radius + pair[1].radius);
            }
        }
    }
    weighted / length
}

fn smooth(points: &mut [Point]) {
    if points.len() < 3 {
        return;
    }
    let radii = points.iter().map(|point| point.radius).collect::<Vec<_>>();
    for index in 1..points.len() - 1 {
        points[index].radius =
            0.25 * radii[index - 1] + 0.5 * radii[index] + 0.25 * radii[index + 1];
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
