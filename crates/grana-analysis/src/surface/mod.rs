mod assemble;
mod bits;
mod cap;
mod cut;
mod distance;
mod emit;
mod label;
mod mask;
mod prepare;
mod tile;

use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;

use grana_geometry::{Geometry, GeometryStore};
use grana_object::SurfaceSet;
use grana_volume::{Intensity, Level, Pyramid, Region, VoxelData};

use crate::morphology::Element;
use grana_object::Mask;
use grana_object::SurfaceCut;
use grana_object::SurfaceParams;

use assemble::Assembler;
use distance::Distance;
use mask::SolidMask;
use tile::{Tile, TileResult};

const TILE_SIDE: u32 = 96;
const DISTANCE_LIMIT: u32 = 12;
const DISTANCE_LIMIT_MAX: u32 = 48;
const GRAIN_LIMIT: f32 = 3.0;
const GRAIN_SUPPORT: f32 = 3.0;
const ANISOTROPY_LIMIT: u32 = 16;
const PAD_ALIGN: u32 = 8;
const ARRAY_BYTES_PER_VOXEL: u64 = 17;
const VALUE_BYTES_PER_VOXEL: u64 = 12;
const WORKER_ARRAY_LIMIT: u64 = 320 << 20;
const FRAME_ARRAY_LIMIT: u64 = 1024 << 20;
const TILE_FRAGMENT_LIMIT: u64 = 96 << 20;
const SEED_SNAP: i64 = 2;

pub const SURFACE_VOXEL_LIMIT: u64 = 1 << 30;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeedPolicy {
    Unseeded,
    Strict,
    Sourced,
}

pub(crate) const EMPTY: u32 = u32::MAX;
pub(crate) const OUTSIDE: f32 = -1.0;
pub(crate) const REMOVED: f32 = 1.0e-3;

pub(crate) type Read<'a> = dyn Fn(usize, &[Range<u32>; 3]) -> Result<VoxelData, String> + Sync + 'a;
pub(crate) type Measure<'a> =
    dyn Fn(usize, &[Range<u32>; 3]) -> Result<VoxelData, String> + Sync + 'a;

pub struct Sources<'a> {
    read: &'a Read<'a>,
    measure: &'a Measure<'a>,
    store: &'a Arc<GeometryStore>,
}

impl<'a> Sources<'a> {
    pub fn new(
        read: &'a Read<'a>,
        measure: &'a Measure<'a>,
        store: &'a Arc<GeometryStore>,
    ) -> Self {
        Self {
            read,
            measure,
            store,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Node {
    pub(crate) parent: u32,
    pub(crate) reach: u32,
    pub(crate) voxels: u64,
    pub(crate) sum: [f64; 3],
    pub(crate) low: [u32; 3],
    pub(crate) high: [u32; 3],
    pub(crate) contact: u32,
    pub(crate) seed: u32,
}

impl Node {
    fn new(reach: u32) -> Self {
        Self {
            parent: 0,
            reach,
            voxels: 0,
            sum: [0.0; 3],
            low: [u32::MAX; 3],
            high: [0; 3],
            contact: EMPTY,
            seed: EMPTY,
        }
    }
}

pub(super) struct Face {
    pub(super) origin: [u32; 2],
    pub(super) dims: [u32; 2],
    pub(super) labels: Vec<u32>,
    pub(super) reach: Vec<u32>,
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

pub(super) struct Gate<'a> {
    pub(super) region: [u32; 3],
    pub(super) limit: [u32; 3],
    pub(super) spacing: [f32; 3],
    pub(super) world: [f32; 3],
    pub(super) roi: Option<&'a Mask>,
}

impl Gate<'_> {
    pub(super) fn admits(&self, voxel: [i64; 3]) -> bool {
        let inside = (0..3).all(|axis| {
            voxel[axis] >= i64::from(self.region[axis]) && voxel[axis] < i64::from(self.limit[axis])
        });
        if !inside {
            return false;
        }
        match self.roi {
            Some(roi) => roi.contains([0, 1, 2].map(|axis| {
                (voxel[axis] as f32 + 0.5) * self.spacing[axis] - 0.5 * self.world[axis]
            })),
            None => true,
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
    anisotropy: [u32; 3],
    kernels: [Vec<f32>; 3],
    opening: Option<Element>,
    closing: Option<Element>,
    morph_radius: u32,
    seeds: Vec<[f32; 3]>,
    seed_policy: SeedPolicy,
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
            anisotropy,
            kernels,
            opening,
            closing,
            morph_radius,
            seeds: Vec::new(),
            seed_policy: SeedPolicy::Unseeded,
        }
    }

    pub fn tile_count(&self) -> usize {
        self.tile_count
    }

    pub fn level(&self) -> usize {
        self.level_index
    }

    pub(super) fn dims(&self) -> [usize; 3] {
        [
            (self.region[0].end - self.region[0].start) as usize,
            (self.region[1].end - self.region[1].start) as usize,
            (self.region[2].end - self.region[2].start) as usize,
        ]
    }

    pub(super) fn blocks(&self) -> [usize; 3] {
        let dims = self.dims();
        dims.map(|value| value.div_ceil(crate::local::BLOCK as usize))
    }

    pub(super) fn tile_position(&self, index: usize) -> [u32; 3] {
        let tiles = self.tiles;
        let x = index % tiles[0] as usize;
        let y = (index / tiles[0] as usize) % tiles[1] as usize;
        let z = index / (tiles[0] as usize * tiles[1] as usize);
        [x as u32, y as u32, z as u32]
    }

    pub(super) fn core_range(&self, index: usize) -> [Range<u32>; 3] {
        assert!(index < self.tile_count, "the tile index is in bounds");
        let position = self.tile_position(index);
        [0, 1, 2].map(|axis| {
            let start = self.region[axis].start + position[axis] * TILE_SIDE;
            start..(start + TILE_SIDE).min(self.region[axis].end)
        })
    }

    pub(super) fn array_range(&self, index: usize, pad: [u32; 3]) -> [Range<u32>; 3] {
        let core = self.core_range(index);
        [0, 1, 2].map(|axis| {
            let start = core[axis].start.saturating_sub(pad[axis]);
            start..(core[axis].end + pad[axis]).min(self.extent[axis])
        })
    }

    pub(super) fn pad_mask(&self) -> [u32; 3] {
        [0, 1, 2].map(|axis| {
            let kernel = (self.kernels[axis].len() / 2) as u32;
            align(kernel + 1) + crate::local::BLOCK as u32 * (crate::local::WINDOW as u32 + 1)
        })
    }

    pub(super) fn pad_morph(&self) -> [u32; 3] {
        [0; 3].map(|_| align(self.morph_radius + 1))
    }

    pub(super) fn gate<'a>(&'a self, roi: Option<&'a Mask>) -> Gate<'a> {
        Gate {
            region: [
                self.region[0].start,
                self.region[1].start,
                self.region[2].start,
            ],
            limit: [self.region[0].end, self.region[1].end, self.region[2].end],
            spacing: self.spacing,
            world: self.world,
            roi,
        }
    }

    pub fn with_seeds(mut self, seeds: &[[f32; 3]], policy: SeedPolicy) -> Self {
        assert!(
            seeds
                .iter()
                .all(|seed| seed.iter().all(|value| value.is_finite())),
            "a surface seed point is finite"
        );
        self.seeds = seeds.to_vec();
        self.seed_policy = policy;
        self
    }

    fn landing(&self, solid: &SolidMask, seed: [f32; 3]) -> Option<[i64; 3]> {
        let centre = [0, 1, 2].map(|axis| {
            ((seed[axis] + 0.5 * self.world[axis]) / self.spacing[axis] - 0.5).round() as i64
        });
        let mut nearest: Option<(i64, [i64; 3])> = None;
        for dz in -SEED_SNAP..=SEED_SNAP {
            for dy in -SEED_SNAP..=SEED_SNAP {
                for dx in -SEED_SNAP..=SEED_SNAP {
                    let voxel = [centre[0] + dx, centre[1] + dy, centre[2] + dz];
                    if !solid.get(voxel, self.anchor) {
                        continue;
                    }
                    let distance = dx * dx + dy * dy + dz * dz;
                    if nearest.is_none_or(|(current, _)| distance < current) {
                        nearest = Some((distance, voxel));
                    }
                }
            }
        }
        nearest.map(|(_, voxel)| voxel)
    }

    fn seeds(&self, solid: &SolidMask) -> Result<Option<Vec<[i64; 3]>>, String> {
        if self.seed_policy == SeedPolicy::Unseeded {
            return Ok(Some(Vec::new()));
        }
        let strict = self.seed_policy == SeedPolicy::Strict;
        let mut landed = Vec::with_capacity(self.seeds.len());
        let mut seen = std::collections::HashSet::new();
        for (index, seed) in self.seeds.iter().enumerate() {
            let Some(voxel) = self.landing(solid, *seed) else {
                if strict {
                    return Err(format!(
                        "the seed point {} does not mark a solid surface voxel",
                        index + 1
                    ));
                }
                continue;
            };
            if !seen.insert(voxel) {
                if strict {
                    return Err(format!(
                        "the seed point {} marks the voxel of another seed point",
                        index + 1
                    ));
                }
                continue;
            }
            landed.push(voxel);
        }
        Ok((!landed.is_empty()).then_some(landed))
    }

    pub fn scan(
        self,
        workers: usize,
        cancel: &AtomicBool,
        roi: Option<&Mask>,
        progress: &mut dyn FnMut(usize, usize),
        sources: Sources<'_>,
    ) -> Result<Option<SurfaceSet>, String> {
        let desired = crate::schedule::workers(workers);
        let store = sources.store;
        let Some(mask) = mask::build(&self, desired, cancel, roi, progress, sources.read)? else {
            return Ok(None);
        };
        let Some(seeds) = self.seeds(&mask.solid)? else {
            return Ok(None);
        };
        let seeds = &seeds[..];
        let widest = self
            .anisotropy
            .iter()
            .copied()
            .max()
            .expect("three axes exist");
        let mut base = (DISTANCE_LIMIT * widest).min(DISTANCE_LIMIT_MAX);
        while base > DISTANCE_LIMIT && Pass::new(&self, base, seeds).max_workers().is_none() {
            base /= 2;
        }
        let mut runs = Vec::new();
        for limit in [base, base * 2, base * 4] {
            match Pass::new(&self, limit, seeds).max_workers() {
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
            progress,
            read: sources.read,
            measure: sources.measure,
            solid: &mask.solid,
            field: &mask.field,
            roi,
        };
        let mut step = 0;
        loop {
            let (limit, capacity) = runs[step];
            let last = step + 1 == runs.len();
            let pass = Pass::new(&self, limit, seeds);
            let workers = desired.min(capacity).min(pass.total().max(1));
            match run_pass(&pass, workers, false, &mut inputs)? {
                PassOutcome::Cancelled => return Ok(None),
                PassOutcome::Saturated if last => {
                    let distance = Distance::build(&self, &mask.solid);
                    let pass = Pass::refined(&self, limit, seeds, &distance);
                    let workers = desired.min(capacity).min(pass.total().max(1));
                    return match run_pass(&pass, workers, false, &mut inputs)? {
                        PassOutcome::Cancelled => Ok(None),
                        PassOutcome::Done(topology) => self.finish(*topology, desired, store),
                        PassOutcome::Saturated => Err(format!(
                            "the refined surface pass keeps a saturated distance of {limit} voxels"
                        )),
                    };
                }
                PassOutcome::Saturated => step += 1,
                PassOutcome::Done(topology) => return self.finish(*topology, desired, store),
            }
        }
    }

    fn finish(
        &self,
        topology: grana_object::SurfaceTopology,
        workers: usize,
        store: &Arc<GeometryStore>,
    ) -> Result<Option<SurfaceSet>, String> {
        let topology = Geometry::paged(store, topology).map_err(|error| error.to_string())?;
        let loaded = topology.load().map_err(|error| error.to_string())?;
        let set = cut::cut(&loaded, self.params.cut(), &[], workers, Some(store))?;
        Ok(Some(
            set.with_topology(topology)
                .map_err(|error| error.to_string())?,
        ))
    }
}

pub fn surface_level(pyramid: &Pyramid, region: Region) -> Option<usize> {
    pyramid.fit_level(region, SURFACE_VOXEL_LIMIT)
}

pub fn recut(
    set: &SurfaceSet,
    spec: SurfaceCut,
    workers: usize,
    store: Option<&Arc<GeometryStore>>,
) -> Result<Option<SurfaceSet>, String> {
    let Some(topology) = set.topology() else {
        return Ok(None);
    };
    let workers = crate::schedule::workers(workers);
    let loaded = topology.load().map_err(|error| error.to_string())?;
    let cut = cut::cut(&loaded, spec, set.removed(), workers, store)?
        .with_topology(topology.clone())
        .map_err(|error| error.to_string())?;
    Ok(Some(cut))
}

pub(super) fn smooth(raw: &mut [f32], dims: [usize; 3], kernels: &[Vec<f32>; 3]) {
    let strides = strides_of(dims);
    for axis in 0..3 {
        let kernel = &kernels[axis];
        if kernel.len() <= 1 {
            continue;
        }
        let radius = (kernel.len() / 2) as i64;
        let (first, second) = ((axis + 1) % 3, (axis + 2) % 3);
        let length = dims[axis];
        let mut line = vec![0.0f32; length];
        let mut blurred = vec![0.0f32; length];
        for position in 0..dims[second] {
            for row in 0..dims[first] {
                let base = strides[first] * row + strides[second] * position;
                for (step, slot) in line.iter_mut().enumerate() {
                    *slot = raw[base + strides[axis] * step];
                }
                for (step, slot) in blurred.iter_mut().enumerate() {
                    let mut sum = 0.0;
                    for (tap, weight) in kernel.iter().enumerate() {
                        let sample = (step as i64 + tap as i64 - radius).clamp(0, length as i64 - 1)
                            as usize;
                        sum += line[sample] * weight;
                    }
                    *slot = sum;
                }
                for (step, value) in blurred.iter().enumerate() {
                    raw[base + strides[axis] * step] = *value;
                }
            }
        }
    }
}

struct Pass<'a> {
    plan: &'a SurfaceBuilder,
    seeds: &'a [[i64; 3]],
    cap: u32,
    pad: [u32; 3],
    values: [u32; 3],
    distance: Option<&'a Distance>,
}

impl<'a> Pass<'a> {
    fn new(plan: &'a SurfaceBuilder, limit: u32, seeds: &'a [[i64; 3]]) -> Self {
        Self::with_distance(plan, limit, seeds, None)
    }

    fn refined(
        plan: &'a SurfaceBuilder,
        limit: u32,
        seeds: &'a [[i64; 3]],
        distance: &'a Distance,
    ) -> Self {
        Self::with_distance(plan, limit, seeds, Some(distance))
    }

    fn with_distance(
        plan: &'a SurfaceBuilder,
        limit: u32,
        seeds: &'a [[i64; 3]],
        distance: Option<&'a Distance>,
    ) -> Self {
        let cap = limit * limit;
        let radius = [0, 1, 2].map(|axis| (plan.kernels[axis].len() / 2) as u32);
        let pad = [0, 1, 2].map(|axis| {
            let needed = limit + radius[axis] + 1;
            align(needed.max(crate::local::BLOCK as u32 * (crate::local::WINDOW as u32 + 1)))
        });
        let values = [0, 1, 2].map(|axis| align(radius[axis] + 3));
        Self {
            plan,
            seeds,
            cap,
            pad,
            values,
            distance,
        }
    }

    fn total(&self) -> usize {
        self.plan.tile_count
    }

    fn array_bytes(&self) -> u64 {
        let plan = self.plan;
        let extent = |pad: [u32; 3]| {
            let mut voxels = 1u64;
            for (axis, pad) in pad.into_iter().enumerate() {
                let size = (plan.region[axis].end - plan.region[axis].start) as u64;
                let span = u64::from(TILE_SIDE + 2 * pad);
                voxels *= size.min(span);
            }
            voxels
        };
        extent(self.pad) * ARRAY_BYTES_PER_VOXEL + extent(self.values) * VALUE_BYTES_PER_VOXEL
    }

    fn max_workers(&self) -> Option<usize> {
        let bytes = self.array_bytes();
        if bytes > WORKER_ARRAY_LIMIT {
            return None;
        }
        Some((FRAME_ARRAY_LIMIT / bytes).clamp(1, crate::schedule::WORKER_LIMIT as u64) as usize)
    }
}

enum PassOutcome {
    Done(Box<grana_object::SurfaceTopology>),
    Saturated,
    Cancelled,
}

struct Inputs<'a> {
    cancel: &'a AtomicBool,
    progress: &'a mut dyn FnMut(usize, usize),
    read: &'a Read<'a>,
    measure: &'a Measure<'a>,
    solid: &'a SolidMask,
    field: &'a crate::local::BlockField,
    roi: Option<&'a Mask>,
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
    let read = inputs.read;
    let measure = inputs.measure;
    let solid = inputs.solid;
    let field = inputs.field;
    let gate = pass.plan.gate(inputs.roi);
    std::thread::scope(|scope| {
        let next = &next;
        let stop = &stop;
        for _ in 0..workers {
            let sender = sender.clone();
            let gate = &gate;
            scope.spawn(move || {
                let mut tile = Tile::new(pass, solid, field, gate);
                loop {
                    if halted(cancel, stop) {
                        break;
                    }
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    if index >= total {
                        break;
                    }
                    let result = tile.run(index, cancel, stop, read, measure);
                    if sender.send((index, result)).is_err() {
                        break;
                    }
                }
            });
        }
        drop(sender);
        assemble(pass, receiver, cancel, stop, accept, &mut *inputs.progress)
    })
}

fn assemble(
    pass: &Pass<'_>,
    receiver: mpsc::Receiver<(usize, TileResult)>,
    cancel: &AtomicBool,
    stop: &AtomicBool,
    accept: bool,
    progress: &mut dyn FnMut(usize, usize),
) -> Result<PassOutcome, String> {
    let total = pass.total();
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
                    assembler.push(assembled, *output)?;
                    assembled += 1;
                    if (assembled - 1).is_multiple_of(4) || assembled == total {
                        progress(pass.total() + assembled, pass.total() * 2);
                    }
                }
            }
        }
    }
    Ok(PassOutcome::Done(Box::new(assembler.topology())))
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
