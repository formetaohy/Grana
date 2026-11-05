use std::ops::Range;
use std::sync::atomic::AtomicBool;

use grana_geometry::MeshVertex;
use grana_object::SampleAccumulator;

use crate::hash::Map;

use super::band::Band;
use super::mask::SolidMask;
use super::{EMPTY, Face, Gate, Measure, Node, Pass, Read, dims_of, strides_of};
use crate::local::BlockField;

pub(super) struct Contact {
    pub(super) object: u32,
    pub(super) labels: [u32; 8],
    pub(super) count: u8,
}

pub(super) struct TileOutput {
    pub(super) position: [u32; 3],
    pub(super) nodes: Vec<Node>,
    pub(super) accumulators: Vec<SampleAccumulator>,
    pub(super) necks: Vec<(u32, u32, u32)>,
    pub(super) seeds: Vec<(u32, u32)>,
    pub(super) vertices: Vec<MeshVertex>,
    pub(super) keys: Vec<u64>,
    pub(super) indices: Vec<u32>,
    pub(super) triangles: Vec<u32>,
    pub(super) contacts: Vec<Contact>,
    pub(super) join: [Option<Face>; 3],
    pub(super) cache: [Option<Face>; 3],
}

pub(super) enum TileResult {
    Finished(Box<TileOutput>),
    Cancelled,
    Failed(String),
}

pub(super) struct Tile<'a> {
    pub(super) pass: &'a Pass<'a>,
    pub(super) solid: &'a SolidMask,
    pub(super) grid: &'a BlockField,
    pub(super) gate: &'a Gate<'a>,
    pub(super) band: Option<Band>,
    pub(super) array: [Range<u32>; 3],
    pub(super) dims: [usize; 3],
    pub(super) start: [i64; 3],
    pub(super) values: [Range<u32>; 3],
    pub(super) value_dims: [usize; 3],
    pub(super) value_start: [i64; 3],
    pub(super) raw: Vec<f32>,
    pub(super) smoothed: Vec<f32>,
    pub(super) field: Vec<f32>,
    pub(super) mask: Vec<bool>,
    pub(super) chamfer: Vec<u32>,
    pub(super) labels: Vec<u32>,
    pub(super) order: Vec<u32>,
    pub(super) order_scratch: Vec<u32>,
    pub(super) nodes: Vec<Node>,
    pub(super) accumulators: Vec<SampleAccumulator>,
    pub(super) necks: Vec<(u32, u32, u32)>,
    pub(super) seeds: Vec<(u32, u32)>,
    pub(super) vertices: Vec<MeshVertex>,
    pub(super) keys: Vec<u64>,
    pub(super) indices: Vec<u32>,
    pub(super) triangles: Vec<u32>,
    pub(super) contacts: Vec<Contact>,
    pub(super) edges: Map<u64, u32>,
    pub(super) space: ([usize; 3], [usize; 3]),
}

impl<'a> Tile<'a> {
    pub(super) fn new(
        pass: &'a Pass<'a>,
        solid: &'a SolidMask,
        grid: &'a BlockField,
        gate: &'a Gate<'a>,
    ) -> Self {
        Self {
            pass,
            solid,
            grid,
            gate,
            band: None,
            array: [0..0, 0..0, 0..0],
            dims: [0; 3],
            start: [0; 3],
            values: [0..0, 0..0, 0..0],
            value_dims: [0; 3],
            value_start: [0; 3],
            raw: Vec::new(),
            smoothed: Vec::new(),
            field: Vec::new(),
            mask: Vec::new(),
            chamfer: Vec::new(),
            labels: Vec::new(),
            order: Vec::new(),
            order_scratch: Vec::new(),
            nodes: Vec::new(),
            accumulators: Vec::new(),
            necks: Vec::new(),
            seeds: Vec::new(),
            vertices: Vec::new(),
            keys: Vec::new(),
            indices: Vec::new(),
            triangles: Vec::new(),
            contacts: Vec::new(),
            edges: Map::default(),
            space: ([0; 3], [0; 3]),
        }
    }

    pub(super) fn run(
        &mut self,
        index: usize,
        cancel: &AtomicBool,
        stop: &AtomicBool,
        read: &Read<'_>,
        measure: &Measure<'_>,
    ) -> TileResult {
        let pass = self.pass;
        let core = pass.plan.core_range(index);
        let array = pass.plan.array_range(index, pass.pad(index));
        self.array = array.clone();
        self.dims = dims_of(&array);
        self.start = [
            array[0].start as i64,
            array[1].start as i64,
            array[2].start as i64,
        ];
        let count = self.dims[0] * self.dims[1] * self.dims[2];
        self.mask.clear();
        self.mask.resize(count, false);
        let occupied = self.mark();
        self.nodes.clear();
        self.accumulators.clear();
        self.necks.clear();
        self.seeds.clear();
        self.vertices.clear();
        self.keys.clear();
        self.indices.clear();
        self.triangles.clear();
        self.contacts.clear();
        self.labels.clear();
        self.chamfer.clear();
        if occupied {
            self.labels.resize(count, EMPTY);
            let inside = [0, 1, 2].map(|axis| {
                let first = pass.plan.region[axis].start as i64 - self.start[axis];
                let last = pass.plan.region[axis].end as i64 - self.start[axis];
                let low = first.clamp(0, self.dims[axis] as i64) as usize;
                let high = last.clamp(0, self.dims[axis] as i64) as usize;
                low..high
            });
            self.band = Some(Band::new(&self.mask, self.dims, &inside));
            let values = pass.plan.array_range(index, pass.values);
            self.values = values.clone();
            self.value_dims = dims_of(&values);
            self.value_start = [
                values[0].start as i64,
                values[1].start as i64,
                values[2].start as i64,
            ];
            let value_count = self.value_dims[0] * self.value_dims[1] * self.value_dims[2];
            let detection = match read(index, &values) {
                Ok(data) => data,
                Err(detail) => return TileResult::Failed(detail),
            };
            if detection.voxel_count() != value_count {
                return TileResult::Failed(
                    "the detection tile does not cover the requested region".to_string(),
                );
            }
            self.raw.clear();
            self.raw.resize(value_count, 0.0);
            detection.write_values(&mut self.raw);
            self.smoothed.clear();
            self.smoothed.extend_from_slice(&self.raw);
            super::smooth(&mut self.smoothed, self.value_dims, &pass.plan.kernels);
            if super::halted(cancel, stop) {
                return TileResult::Cancelled;
            }
            self.field.clear();
            self.field.resize(value_count, 0.0);
            self.field(&core);
            self.watershed();
            self.record_necks();
            if super::halted(cancel, stop) {
                return TileResult::Cancelled;
            }
            for (seed, position) in pass.seeds.iter().enumerate() {
                let inside = (0..3).all(|axis| {
                    position[axis] >= core[axis].start as i64
                        && position[axis] < core[axis].end as i64
                });
                if !inside {
                    continue;
                }
                if let Some(label) = self.seed_label(*position) {
                    self.seeds.push((seed as u32, label));
                }
            }
            let nodes_bytes = self.nodes.len() as u64 * std::mem::size_of::<Node>() as u64
                + self.accumulators.len() as u64 * std::mem::size_of::<SampleAccumulator>() as u64;
            if nodes_bytes > super::TILE_FRAGMENT_LIMIT {
                return TileResult::Failed(format!(
                    "the surface tile {index} exceeds the {} byte fragment limit",
                    super::TILE_FRAGMENT_LIMIT
                ));
            }
            if let Err(detail) = self.accumulate(&core, &array, measure) {
                return TileResult::Failed(detail);
            }
            if super::halted(cancel, stop) {
                return TileResult::Cancelled;
            }
            if let Err(detail) = self.emit(&core) {
                return TileResult::Failed(detail);
            }
        }
        let position = pass.plan.tile_position(index);
        let join = [0, 1, 2].map(|axis| {
            (position[axis] > 0)
                .then(|| self.slice(axis, (core[axis].start - array[axis].start) as usize))
        });
        let cache = [0, 1, 2].map(|axis| {
            (core[axis].end < array[axis].end)
                .then(|| self.slice(axis, (core[axis].end - array[axis].start) as usize))
        });
        let output = TileOutput {
            position,
            nodes: std::mem::take(&mut self.nodes),
            accumulators: std::mem::take(&mut self.accumulators),
            necks: std::mem::take(&mut self.necks),
            seeds: std::mem::take(&mut self.seeds),
            vertices: std::mem::take(&mut self.vertices),
            keys: std::mem::take(&mut self.keys),
            indices: std::mem::take(&mut self.indices),
            triangles: std::mem::take(&mut self.triangles),
            contacts: std::mem::take(&mut self.contacts),
            join,
            cache,
        };
        let bytes = output_bytes(&output);
        if bytes > super::TILE_FRAGMENT_LIMIT {
            return TileResult::Failed(format!(
                "the surface tile {index} exceeds the {bytes} byte fragment limit"
            ));
        }
        TileResult::Finished(Box::new(output))
    }

    pub(super) fn fragment_bytes(&self) -> u64 {
        self.vertices.len() as u64 * 32
            + self.keys.len() as u64 * 8
            + self.indices.len() as u64 * 4
            + self.triangles.len() as u64 * 4
    }

    pub(super) fn create_node(&mut self, reach: u32) -> u32 {
        let node = self.nodes.len() as u32;
        assert_ne!(
            node, EMPTY,
            "a tile holds fewer nodes than the empty sentinel"
        );
        self.nodes.push(Node {
            parent: node,
            ..Node::new(reach)
        });
        for _ in 0..self.pass.plan.channels {
            self.accumulators.push(SampleAccumulator::new());
        }
        node
    }

    pub(super) fn value_index(&self, voxel: [i64; 3]) -> Option<usize> {
        let dims = self.value_dims;
        let mut local = [0usize; 3];
        for axis in 0..3 {
            let value = voxel[axis] - self.value_start[axis];
            if value < 0 || value >= dims[axis] as i64 {
                return None;
            }
            local[axis] = value as usize;
        }
        Some(local[0] + dims[0] * (local[1] + dims[1] * local[2]))
    }

    pub(super) fn mask_index(&self, voxel: [i64; 3]) -> Option<usize> {
        let dims = self.dims;
        let mut local = [0usize; 3];
        for axis in 0..3 {
            let value = voxel[axis] - self.start[axis];
            if value < 0 || value >= dims[axis] as i64 {
                return None;
            }
            local[axis] = value as usize;
        }
        Some(local[0] + dims[0] * (local[1] + dims[1] * local[2]))
    }

    fn seed_label(&self, seed: [i64; 3]) -> Option<u32> {
        let mut nearest: Option<(i64, u32)> = None;
        for dz in -super::SEED_SNAP..=super::SEED_SNAP {
            for dy in -super::SEED_SNAP..=super::SEED_SNAP {
                for dx in -super::SEED_SNAP..=super::SEED_SNAP {
                    let voxel = [seed[0] + dx, seed[1] + dy, seed[2] + dz];
                    let Some(slot) = self.mask_index(voxel) else {
                        continue;
                    };
                    let label = self.labels[slot];
                    if label == EMPTY {
                        continue;
                    }
                    let distance = dx * dx + dy * dy + dz * dz;
                    if nearest.is_none_or(|(current, _)| distance < current) {
                        nearest = Some((distance, label));
                    }
                }
            }
        }
        nearest.map(|(_, label)| label)
    }

    fn slice(&self, axis: usize, plane: usize) -> Face {
        let dims = self.dims;
        let strides = strides_of(dims);
        let array = self.array.clone();
        let first = (axis + 1) % 3;
        let second = (axis + 2) % 3;
        let mut labels = Vec::with_capacity(dims[first] * dims[second]);
        let mut reach = Vec::with_capacity(dims[first] * dims[second]);
        for second_position in 0..dims[second] {
            for first_position in 0..dims[first] {
                let slot = strides[axis] * plane
                    + strides[first] * first_position
                    + strides[second] * second_position;
                labels.push(if self.labels.is_empty() {
                    EMPTY
                } else {
                    self.labels[slot]
                });
                reach.push(if self.chamfer.is_empty() {
                    0
                } else {
                    self.chamfer[slot]
                });
            }
        }
        Face {
            origin: [array[first].start, array[second].start],
            dims: [dims[first] as u32, dims[second] as u32],
            labels,
            reach,
        }
    }
}

fn output_bytes(output: &TileOutput) -> u64 {
    let nodes = output.nodes.len() as u64 * std::mem::size_of::<Node>() as u64;
    let accumulators =
        output.accumulators.len() as u64 * std::mem::size_of::<SampleAccumulator>() as u64;
    let necks = output.necks.len() as u64 * 12;
    let seeds = output.seeds.len() as u64 * 8;
    let vertices = output.vertices.len() as u64 * std::mem::size_of::<MeshVertex>() as u64;
    let keys = output.keys.len() as u64 * 8;
    let indices = output.indices.len() as u64 * 4;
    let triangles = output.triangles.len() as u64 * 4;
    let contacts = output.contacts.len() as u64 * std::mem::size_of::<Contact>() as u64;
    let faces = [&output.join, &output.cache]
        .into_iter()
        .flatten()
        .filter_map(|face| face.as_ref())
        .map(|face| face.labels.len() as u64 * 8)
        .sum::<u64>();
    nodes + accumulators + necks + seeds + vertices + keys + indices + triangles + contacts + faces
}
