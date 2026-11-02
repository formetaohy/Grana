mod emit;
mod finish;
mod label;
mod prepare;

use std::collections::HashMap;
use std::ops::Range;

use grana_object::{SampleAccumulator, SurfaceSet};
use grana_volume::{Intensity, Level, Region, VoxelData};

use crate::SurfaceParams;
use crate::mask::Mask;
use crate::morphology::Element;

const TILE_SIDE: u32 = 96;
const DISTANCE_LIMIT: u32 = 12;
const GRAIN_LIMIT: f32 = 3.0;
const GRAIN_SUPPORT: f32 = 3.0;
const ANISOTROPY_LIMIT: u32 = 16;
const BLOCK_PAD: u32 = crate::local::PAD_BLOCKS as u32 * crate::local::BLOCK as u32;
const PAD_ALIGN: u32 = crate::local::BLOCK as u32;
pub(crate) const EMPTY: u32 = u32::MAX;
pub(crate) const OUTSIDE: f32 = -1.0;
pub(crate) const REMOVED: f32 = 1.0e-3;

#[derive(Clone, Copy)]
pub(crate) struct Node {
    pub(crate) parent: u32,
    pub(crate) rank: u8,
    pub(crate) reach: u32,
    pub(crate) voxels: u64,
    pub(crate) sum: [f64; 3],
    pub(crate) low: [u32; 3],
    pub(crate) high: [u32; 3],
    pub(crate) contact: u32,
}

impl Node {
    fn new(reach: u32) -> Self {
        Self {
            parent: 0,
            rank: 0,
            reach,
            voxels: 0,
            sum: [0.0; 3],
            low: [u32::MAX; 3],
            high: [0; 3],
            contact: EMPTY,
        }
    }
}

pub(crate) struct Face {
    pub(crate) origin: [u32; 2],
    pub(crate) dims: [u32; 2],
    pub(crate) labels: Vec<u32>,
}

pub struct SurfaceBuilder {
    level_index: usize,
    detection: usize,
    channels: usize,
    extent: [u32; 3],
    spacing: [f32; 3],
    world: [f32; 3],
    voxel_volume: f64,
    span: f32,
    intensity: Intensity,
    params: SurfaceParams,
    threshold: f32,
    region: [Range<u32>; 3],
    anchor: [i64; 3],
    tiles: [u32; 3],
    tile_count: usize,
    next: usize,
    split: bool,
    merge_gap: u32,
    anisotropy: [u32; 3],
    distance_cap: u16,
    kernels: [Vec<f32>; 3],
    opening: Option<Element>,
    closing: Option<Element>,
    pad: [u32; 3],
    start: [i64; 3],
    dims: [usize; 3],
    raw: Vec<f32>,
    field: Vec<f32>,
    mask: Vec<bool>,
    scratch: Vec<bool>,
    chamfer: Vec<u16>,
    labels: Vec<u32>,
    order: Vec<u32>,
    stack: Vec<u32>,
    nodes: Vec<Node>,
    accumulators: Vec<SampleAccumulator>,
    necks: Vec<(u32, u32, u16)>,
    vertices: Vec<grana_geometry::MeshVertex>,
    keys: Vec<u64>,
    indices: Vec<u32>,
    triangle_objects: Vec<u32>,
    edges: HashMap<u64, u32>,
    x_face: Option<Face>,
    y_faces: Vec<Option<Face>>,
    z_faces: Vec<Option<Face>>,
}

impl SurfaceBuilder {
    pub fn new(
        level_index: usize,
        level: Level,
        params: SurfaceParams,
        channels: usize,
        detection: usize,
        region: Region,
        intensity: Intensity,
    ) -> Self {
        assert!(
            channels > 0 && detection < channels,
            "the detection channel is one of the measured channels"
        );
        let extent = level.extent();
        let extent = [extent.x(), extent.y(), extent.z()];
        let spacing = level.spacing();
        let spacing = [spacing.x(), spacing.y(), spacing.z()];
        let world = level.world_extent();
        let voxel_volume = f64::from(spacing[0]) * f64::from(spacing[1]) * f64::from(spacing[2]);
        let region = region.voxel_range(level);
        let region = region.unwrap_or([0..0, 0..0, 0..0]);
        let narrowest = spacing.iter().copied().fold(f32::INFINITY, f32::min);
        let grain = params.grain().min(GRAIN_LIMIT * narrowest);
        let anisotropy =
            spacing.map(|value| ((value / narrowest).round() as u32).clamp(1, ANISOTROPY_LIMIT));
        let split = params.seed() > 0.0;
        let reach = if split { DISTANCE_LIMIT } else { 0 };
        let widest = anisotropy.iter().copied().max().expect("three axes exist");
        let distance_cap = ((reach * widest) * (reach * widest)) as u16;
        let merge_gap = (params.seed() / (2.0 * narrowest)).round().max(1.0) as u32;
        let kernels = spacing.map(|value| gaussian((grain / value).min(GRAIN_LIMIT)));
        let opening = Element::new(level, params.opening());
        let closing = Element::new(level, params.closing());
        let radius = [opening.as_ref(), closing.as_ref()]
            .into_iter()
            .flatten()
            .map(|element| element.reach())
            .fold(0, u32::max);
        let pad = [0, 1, 2].map(|axis| {
            let needed = reach + (kernels[axis].len() / 2) as u32 + 1;
            align(needed.max(radius + 1).max(BLOCK_PAD))
        });
        let anchor = [
            region[0].start as i64,
            region[1].start as i64,
            region[2].start as i64,
        ];
        let tiles = [0, 1, 2].map(|axis| {
            let count = region[axis].end.saturating_sub(region[axis].start);
            count.div_ceil(TILE_SIDE)
        });
        let tile_count = tiles[0] as usize * tiles[1] as usize * tiles[2] as usize;
        let y_faces = (0..tiles[0]).map(|_| None).collect();
        let z_faces = (0..tiles[0] * tiles[1]).map(|_| None).collect();
        Self {
            level_index,
            detection,
            channels,
            extent,
            spacing,
            world,
            voxel_volume,
            span: intensity.span(),
            intensity,
            params,
            threshold: params.threshold(),
            region,
            anchor,
            tiles,
            tile_count,
            next: 0,
            split,
            merge_gap,
            anisotropy,
            distance_cap,
            kernels,
            opening,
            closing,
            pad,
            start: [0; 3],
            dims: [0; 3],
            raw: Vec::new(),
            field: Vec::new(),
            mask: Vec::new(),
            scratch: Vec::new(),
            chamfer: Vec::new(),
            labels: Vec::new(),
            order: Vec::new(),
            stack: Vec::new(),
            nodes: Vec::new(),
            accumulators: Vec::new(),
            necks: Vec::new(),
            vertices: Vec::new(),
            keys: Vec::new(),
            indices: Vec::new(),
            triangle_objects: Vec::new(),
            edges: HashMap::new(),
            x_face: None,
            y_faces,
            z_faces,
        }
    }

    pub fn tile_count(&self) -> usize {
        self.tile_count
    }

    pub fn core_range(&self, index: usize) -> [Range<u32>; 3] {
        assert!(index < self.tile_count, "the tile index is in bounds");
        let [x, y, z] = self.tile_position(index);
        [0, 1, 2].map(|axis| {
            let start = self.region[axis].start + [x, y, z][axis] * TILE_SIDE;
            start..(start + TILE_SIDE).min(self.region[axis].end)
        })
    }

    pub fn detection_range(&self, index: usize) -> [Range<u32>; 3] {
        let core = self.core_range(index);
        [0, 1, 2].map(|axis| {
            let start = core[axis].start.saturating_sub(self.pad[axis]);
            start..(core[axis].end + self.pad[axis]).min(self.extent[axis])
        })
    }

    pub fn add(
        &mut self,
        index: usize,
        detection: &VoxelData,
        roi: Option<&Mask>,
        measure: impl FnMut(usize, &[Range<u32>; 3]) -> Result<VoxelData, String>,
    ) -> Result<(), String> {
        assert_eq!(index, self.next, "surface tiles arrive in raster order");
        let core = self.core_range(index);
        let array = self.detection_range(index);
        self.dims = dims_of(&array);
        self.start = [
            array[0].start as i64,
            array[1].start as i64,
            array[2].start as i64,
        ];
        let count = self.dims[0] * self.dims[1] * self.dims[2];
        if detection.voxel_count() != count {
            return Err("the detection tile does not cover the requested region".to_string());
        }
        self.raw.clear();
        self.raw.resize(count, 0.0);
        detection.write_values(&mut self.raw);
        self.smooth();
        self.prepare(roi);
        self.labels.clear();
        self.labels.resize(count, EMPTY);
        if self.split {
            self.watershed();
            self.record_necks();
        } else {
            self.flood();
        }
        self.join();
        self.accumulate(&core, &array, measure)?;
        self.store_faces(&core, &array);
        self.emit();
        self.next += 1;
        Ok(())
    }

    pub fn finish(mut self) -> SurfaceSet {
        assert_eq!(
            self.next, self.tile_count,
            "every surface tile has been added"
        );
        self.merge_basins();
        self.build()
    }

    fn tile_position(&self, index: usize) -> [u32; 3] {
        let x = index % self.tiles[0] as usize;
        let y = (index / self.tiles[0] as usize) % self.tiles[1] as usize;
        let z = index / (self.tiles[0] as usize * self.tiles[1] as usize);
        [x as u32, y as u32, z as u32]
    }

    fn array_range(&self) -> [Range<u32>; 3] {
        self.detection_range(self.next)
    }

    fn array_index(&self, voxel: [i64; 3]) -> Option<usize> {
        let mut local = [0usize; 3];
        for axis in 0..3 {
            let value = voxel[axis] - self.start[axis];
            if value < 0 || value >= self.dims[axis] as i64 {
                return None;
            }
            local[axis] = value as usize;
        }
        Some(local[0] + self.dims[0] * (local[1] + self.dims[1] * local[2]))
    }

    fn create_node(&mut self, reach: u32) -> u32 {
        let node = self.nodes.len() as u32;
        self.nodes.push(Node {
            parent: node,
            ..Node::new(reach)
        });
        for _ in 0..self.channels {
            self.accumulators.push(SampleAccumulator::new());
        }
        node
    }

    fn union(&mut self, left: u32, right: u32) -> u32 {
        let mut left = find(&mut self.nodes, left);
        let mut right = find(&mut self.nodes, right);
        if left == right {
            return left;
        }
        if self.nodes[left as usize].rank < self.nodes[right as usize].rank {
            std::mem::swap(&mut left, &mut right);
        }
        let winner = self.nodes[left as usize];
        let loser = self.nodes[right as usize];
        for channel in 0..self.channels {
            let other = self.accumulators[right as usize * self.channels + channel];
            self.accumulators[left as usize * self.channels + channel].merge(&other);
        }
        let target = &mut self.nodes[left as usize];
        target.reach = target.reach.max(loser.reach);
        target.voxels += loser.voxels;
        for axis in 0..3 {
            target.sum[axis] += loser.sum[axis];
            target.low[axis] = target.low[axis].min(loser.low[axis]);
            target.high[axis] = target.high[axis].max(loser.high[axis]);
        }
        if target.contact == EMPTY {
            target.contact = loser.contact;
        }
        if winner.rank == loser.rank {
            target.rank += 1;
        }
        self.nodes[right as usize].parent = left;
        left
    }
}

pub(crate) fn find(nodes: &mut [Node], node: u32) -> u32 {
    let mut root = node;
    while nodes[root as usize].parent != root {
        root = nodes[root as usize].parent;
    }
    let mut current = node;
    while nodes[current as usize].parent != root {
        let next = nodes[current as usize].parent;
        nodes[current as usize].parent = root;
        current = next;
    }
    root
}

pub(crate) fn gaussian(sigma: f32) -> Vec<f32> {
    let support = (GRAIN_SUPPORT * sigma).ceil() as usize;
    if support == 0 {
        return vec![1.0];
    }
    let mut kernel = (0..=2 * support)
        .map(|index| {
            let offset = index as f32 - support as f32;
            (-0.5 * (offset / sigma) * (offset / sigma)).exp()
        })
        .collect::<Vec<_>>();
    let total: f32 = kernel.iter().sum();
    for weight in &mut kernel {
        *weight /= total;
    }
    kernel
}

pub(crate) fn parabola(line: &mut [f64], weight: f64) {
    let length = line.len();
    if length < 3 {
        return;
    }
    let mut locations = vec![0usize; length];
    let mut boundaries = vec![0.0f64; length + 1];
    let mut envelope = 0usize;
    boundaries[0] = f64::NEG_INFINITY;
    boundaries[1] = f64::INFINITY;
    for position in 1..length {
        let mut boundary = crossing(line, &locations, envelope, position, weight);
        while envelope > 0 && boundary <= boundaries[envelope] {
            envelope -= 1;
            boundary = crossing(line, &locations, envelope, position, weight);
        }
        envelope += 1;
        locations[envelope] = position;
        boundaries[envelope] = boundary;
        boundaries[envelope + 1] = f64::INFINITY;
    }
    let mut envelope = 0usize;
    for position in 0..length {
        while boundaries[envelope + 1] < position as f64 {
            envelope += 1;
        }
        let distance = position as f64 - locations[envelope] as f64;
        line[position] = weight * distance * distance + line[locations[envelope]];
    }
}

fn crossing(
    line: &[f64],
    locations: &[usize],
    envelope: usize,
    position: usize,
    weight: f64,
) -> f64 {
    let anchor = locations[envelope];
    ((line[position] + weight * (position * position) as f64)
        - (line[anchor] + weight * (anchor * anchor) as f64))
        / (2.0 * weight * (position as f64 - anchor as f64))
}

fn align(value: u32) -> u32 {
    value.div_ceil(PAD_ALIGN) * PAD_ALIGN
}

fn dims_of(ranges: &[Range<u32>; 3]) -> [usize; 3] {
    [0, 1, 2].map(|axis| (ranges[axis].end - ranges[axis].start) as usize)
}

pub(crate) fn strides_of(dims: [usize; 3]) -> [usize; 3] {
    [1, dims[0], dims[0] * dims[1]]
}

pub(crate) fn length(squared: u32) -> u32 {
    (squared as f64).sqrt() as u32
}
