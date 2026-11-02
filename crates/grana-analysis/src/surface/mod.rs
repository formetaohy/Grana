mod assemble;
mod emit;
mod finish;
mod label;
mod prepare;
mod tile;

use std::ops::Range;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;

use grana_object::{SampleAccumulator, SurfaceSet};
use grana_volume::{Intensity, Level, Region, VoxelData};

use crate::SurfaceParams;
use crate::mask::Mask;
use crate::morphology::Element;

use assemble::Assembler;
use tile::{Tile, TileResult};

const TILE_SIDE: u32 = 96;
const DISTANCE_LIMIT: u32 = 12;
const DISTANCE_LIMIT_MAX: u32 = 48;
const GRAIN_LIMIT: f32 = 3.0;
const GRAIN_SUPPORT: f32 = 3.0;
const ANISOTROPY_LIMIT: u32 = 16;
const BLOCK_PAD: u32 = crate::local::PAD_BLOCKS as u32 * crate::local::BLOCK as u32;
const PAD_ALIGN: u32 = crate::local::BLOCK as u32;
const ARRAY_BYTES_PER_VOXEL: u64 = 28;
const WORKER_ARRAY_LIMIT: u64 = 320 << 20;
const FRAME_ARRAY_LIMIT: u64 = 1024 << 20;
const TILE_FRAGMENT_LIMIT: u64 = 96 << 20;
const WORKER_LIMIT: usize = 16;

pub(crate) const EMPTY: u32 = u32::MAX;
pub(crate) const OUTSIDE: f32 = -1.0;
pub(crate) const REMOVED: f32 = 1.0e-3;

pub(crate) type Read<'a> = dyn Fn(usize, &[Range<u32>; 3]) -> Result<VoxelData, String> + Sync + 'a;
pub(crate) type Measure<'a> =
    dyn Fn(usize, &[Range<u32>; 3]) -> Result<VoxelData, String> + Sync + 'a;

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

pub(super) struct Face {
    pub(super) origin: [u32; 2],
    pub(super) dims: [u32; 2],
    pub(super) labels: Vec<u32>,
}

impl Face {
    fn rebase(&mut self, base: u32) {
        for label in &mut self.labels {
            if *label != EMPTY {
                *label += base;
            }
        }
    }
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
    split: bool,
    merge_gap: u32,
    anisotropy: [u32; 3],
    kernels: [Vec<f32>; 3],
    opening: Option<Element>,
    closing: Option<Element>,
    morph_radius: u32,
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
        let merge_gap = (params.seed() / (2.0 * narrowest)).round().max(1.0) as u32;
        let kernels = spacing.map(|value| gaussian((grain / value).min(GRAIN_LIMIT)));
        let opening = Element::new(level, params.opening());
        let closing = Element::new(level, params.closing());
        let morph_radius = [opening.as_ref(), closing.as_ref()]
            .into_iter()
            .flatten()
            .map(|element| element.reach())
            .fold(0, u32::max);
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
            split,
            merge_gap,
            anisotropy,
            kernels,
            opening,
            closing,
            morph_radius,
        }
    }

    pub fn tile_count(&self) -> usize {
        self.tile_count
    }

    pub fn scan(
        self,
        workers: usize,
        cancel: &AtomicBool,
        roi: Option<&Mask>,
        progress: &mut dyn FnMut(usize, usize),
        read: &Read<'_>,
        measure: &Measure<'_>,
    ) -> Result<Option<SurfaceSet>, String> {
        let widest = self
            .anisotropy
            .iter()
            .copied()
            .max()
            .expect("three axes exist");
        let mut base = if self.split {
            (DISTANCE_LIMIT * widest).min(DISTANCE_LIMIT_MAX)
        } else {
            0
        };
        while base > DISTANCE_LIMIT && Pass::new(&self, base).max_workers().is_none() {
            base /= 2;
        }
        let desired = match workers {
            0 => std::thread::available_parallelism().map_or(1, |value| value.get()),
            value => value,
        }
        .clamp(1, WORKER_LIMIT);
        let mut runs = Vec::new();
        let limits = if self.split {
            vec![base, base * 2, base * 4]
        } else {
            vec![0]
        };
        for limit in limits {
            match Pass::new(&self, limit).max_workers() {
                Some(capacity) => runs.push((limit, capacity)),
                None => break,
            }
        }
        if runs.is_empty() {
            return Err(format!(
                "a surface tile needs more than {WORKER_ARRAY_LIMIT} bytes of array memory"
            ));
        }
        let mut inputs = Inputs {
            cancel,
            roi,
            progress,
            read,
            measure,
        };
        let mut step = 0;
        loop {
            let (limit, capacity) = runs[step];
            let last = step + 1 == runs.len();
            let pass = Pass::new(&self, limit);
            let workers = desired.min(capacity).min(pass.total().max(1));
            let outcome = run_pass(&pass, workers, last, &mut inputs)?;
            match outcome {
                PassOutcome::Cancelled => return Ok(None),
                PassOutcome::Saturated => {
                    assert!(!last, "the final surface pass accepts a saturated field");
                    step += 1;
                }
                PassOutcome::Done(set) => return Ok(Some(set)),
            }
        }
    }
}

struct Pass<'a> {
    plan: &'a SurfaceBuilder,
    cap: u32,
    pad: [u32; 3],
}

impl<'a> Pass<'a> {
    fn new(plan: &'a SurfaceBuilder, limit: u32) -> Self {
        let cap = limit * limit;
        let pad = [0, 1, 2].map(|axis| {
            let needed = limit + (plan.kernels[axis].len() / 2) as u32 + 1;
            align(needed.max(plan.morph_radius + 1).max(BLOCK_PAD))
        });
        Self { plan, cap, pad }
    }

    fn total(&self) -> usize {
        self.plan.tile_count
    }

    fn tile_position(&self, index: usize) -> [u32; 3] {
        let tiles = self.plan.tiles;
        let x = index % tiles[0] as usize;
        let y = (index / tiles[0] as usize) % tiles[1] as usize;
        let z = index / (tiles[0] as usize * tiles[1] as usize);
        [x as u32, y as u32, z as u32]
    }

    fn core_range(&self, index: usize) -> [Range<u32>; 3] {
        assert!(index < self.total(), "the tile index is in bounds");
        let position = self.tile_position(index);
        [0, 1, 2].map(|axis| {
            let start = self.plan.region[axis].start + position[axis] * TILE_SIDE;
            start..(start + TILE_SIDE).min(self.plan.region[axis].end)
        })
    }

    fn array_range(&self, index: usize) -> [Range<u32>; 3] {
        let core = self.core_range(index);
        [0, 1, 2].map(|axis| {
            let start = core[axis].start.saturating_sub(self.pad[axis]);
            start..(core[axis].end + self.pad[axis]).min(self.plan.extent[axis])
        })
    }

    fn array_bytes(&self) -> u64 {
        let mut voxels = 1u64;
        for axis in 0..3 {
            let size = (self.plan.region[axis].end - self.plan.region[axis].start) as u64;
            let span = u64::from(TILE_SIDE + 2 * self.pad[axis]);
            voxels *= size.min(span);
        }
        voxels * ARRAY_BYTES_PER_VOXEL
    }

    fn max_workers(&self) -> Option<usize> {
        let bytes = self.array_bytes();
        if bytes > WORKER_ARRAY_LIMIT {
            return None;
        }
        Some((FRAME_ARRAY_LIMIT / bytes).clamp(1, WORKER_LIMIT as u64) as usize)
    }
}

enum PassOutcome {
    Done(SurfaceSet),
    Saturated,
    Cancelled,
}

struct Inputs<'a> {
    cancel: &'a AtomicBool,
    roi: Option<&'a Mask>,
    progress: &'a mut dyn FnMut(usize, usize),
    read: &'a Read<'a>,
    measure: &'a Measure<'a>,
}

fn run_pass(
    pass: &Pass<'_>,
    workers: usize,
    accept: bool,
    inputs: &mut Inputs<'_>,
) -> Result<PassOutcome, String> {
    let total = pass.total();
    let next = AtomicUsize::new(0);
    let stop = AtomicBool::new(false);
    let (sender, receiver) = mpsc::sync_channel::<(usize, TileResult)>(workers * 2);
    let cancel = inputs.cancel;
    let roi = inputs.roi;
    let read = inputs.read;
    let measure = inputs.measure;
    std::thread::scope(|scope| {
        let next = &next;
        let stop = &stop;
        for _ in 0..workers {
            let sender = sender.clone();
            scope.spawn(move || {
                let mut tile = Tile::new(pass);
                loop {
                    if halted(cancel, stop) {
                        break;
                    }
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    if index >= total {
                        break;
                    }
                    let result = tile.run(index, cancel, stop, roi, read, measure);
                    if sender.send((index, result)).is_err() {
                        break;
                    }
                }
            });
        }
        drop(sender);
        assemble(
            pass,
            total,
            receiver,
            cancel,
            stop,
            accept,
            &mut *inputs.progress,
        )
    })
}

fn assemble(
    pass: &Pass<'_>,
    total: usize,
    receiver: mpsc::Receiver<(usize, TileResult)>,
    cancel: &AtomicBool,
    stop: &AtomicBool,
    accept: bool,
    progress: &mut dyn FnMut(usize, usize),
) -> Result<PassOutcome, String> {
    let mut assembler = Assembler::new(pass);
    let mut pending: Vec<Option<TileResult>> = (0..total).map(|_| None).collect();
    let mut assembled = 0usize;
    while assembled < total {
        if cancel.load(Ordering::Relaxed) {
            stop.store(true, Ordering::Relaxed);
            return Ok(PassOutcome::Cancelled);
        }
        let (index, result) = match receiver.recv() {
            Ok(value) => value,
            Err(_) => {
                if cancel.load(Ordering::Relaxed) {
                    return Ok(PassOutcome::Cancelled);
                }
                return Err(
                    "the surface workers stopped before every tile was assembled".to_string(),
                );
            }
        };
        pending[index] = Some(result);
        while assembled < total {
            let Some(result) = pending[assembled].take() else {
                break;
            };
            match result {
                TileResult::Cancelled => {
                    stop.store(true, Ordering::Relaxed);
                    return Ok(PassOutcome::Cancelled);
                }
                TileResult::Failed(detail) => {
                    stop.store(true, Ordering::Relaxed);
                    return Err(detail);
                }
                TileResult::Finished(output) => {
                    if !accept && output.saturated {
                        stop.store(true, Ordering::Relaxed);
                        return Ok(PassOutcome::Saturated);
                    }
                    assembler.push(assembled, *output);
                    assembled += 1;
                    if (assembled - 1).is_multiple_of(4) || assembled == total {
                        progress(assembled, total);
                    }
                }
            }
        }
    }
    assembler.merge_basins();
    Ok(PassOutcome::Done(assembler.build()))
}

fn halted(cancel: &AtomicBool, stop: &AtomicBool) -> bool {
    cancel.load(Ordering::Relaxed) || stop.load(Ordering::Relaxed)
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

fn union(
    nodes: &mut [Node],
    accumulators: &mut [SampleAccumulator],
    channels: usize,
    left: u32,
    right: u32,
) -> u32 {
    let mut left = find(nodes, left);
    let mut right = find(nodes, right);
    if left == right {
        return left;
    }
    if nodes[left as usize].rank < nodes[right as usize].rank {
        std::mem::swap(&mut left, &mut right);
    }
    let winner = nodes[left as usize];
    let loser = nodes[right as usize];
    for channel in 0..channels {
        let other = accumulators[right as usize * channels + channel];
        accumulators[left as usize * channels + channel].merge(&other);
    }
    let target = &mut nodes[left as usize];
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
    nodes[right as usize].parent = left;
    left
}

fn gaussian(sigma: f32) -> Vec<f32> {
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

fn parabola(line: &mut [f64], weight: f64) {
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

pub(crate) fn dims_of(ranges: &[Range<u32>; 3]) -> [usize; 3] {
    [0, 1, 2].map(|axis| (ranges[axis].end - ranges[axis].start) as usize)
}

pub(crate) fn strides_of(dims: [usize; 3]) -> [usize; 3] {
    [1, dims[0], dims[0] * dims[1]]
}
