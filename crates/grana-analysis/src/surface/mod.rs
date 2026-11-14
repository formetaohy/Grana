mod assemble;
mod band;
mod bits;
mod cap;
mod cut;
mod detection;
mod distance;
mod edit;
mod emit;
mod label;
mod mask;
mod prepare;
mod tile;

use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;

use grana_geometry::Geometry;
use grana_object::SurfaceSet;
use grana_store::SpillStore;
use grana_volume::{Intensity, Level, Pyramid, Region, VoxelData};

use crate::morphology::Element;
use grana_object::Mask;
use grana_object::SurfaceCut;
use grana_object::SurfaceParams;

use assemble::Assembler;
use detection::{Detection, DetectionBuild, DetectionInput, DetectionSource, DetectionStore};
use distance::Distance;
pub use edit::{apply, merge, split};
use mask::SolidMask;
use tile::{Tile, TileResult};

use crate::schedule::Budget;

const TILE_SIDE: u32 = 96;
const TILE_Z_FLOOR: u32 = 8;
const SLAB_BUDGET: u64 = 256 << 20;
const DETECTION_CACHE: u64 = 768 << 20;
const DISTANCE_LIMIT: u32 = 12;
const DISTANCE_LIMIT_MAX: u32 = 48;
const GRAIN_LIMIT: f32 = 3.0;
const GRAIN_SUPPORT: f32 = 3.0;
const ANISOTROPY_LIMIT: u32 = 16;
const PAD_ALIGN: u32 = 8;
const ARRAY_BYTES_PER_VOXEL: u64 = 9;
const FIELD_BYTES_PER_VOXEL: u64 = 4;
const FIELD_REACH: u32 = 2;
const WORKER_ARRAY_LIMIT: u64 = 320 << 20;
const FRAME_ARRAY_LIMIT: u64 = 1024 << 20;
const TILE_FRAGMENT_LIMIT: u64 = 96 << 20;
const SEED_SNAP: i64 = 2;
const SURFACE_VOXELS: u64 = 1 << 30;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeedPolicy {
    Unseeded,
    Strict,
    Sourced,
}

pub(crate) const EMPTY: u32 = u32::MAX;
pub(crate) const OUTSIDE: f32 = -1.0;
pub(crate) const REMOVED: f32 = 1.0e-3;

pub(crate) type Read<'a> = dyn Fn(&[Range<u32>; 3]) -> Result<VoxelData, String> + Sync + 'a;
pub(crate) type Measure<'a> =
    dyn Fn(usize, &[Range<u32>; 3]) -> Result<VoxelData, String> + Sync + 'a;

pub struct Sources<'a> {
    read: &'a Read<'a>,
    measure: &'a Measure<'a>,
    store: &'a Arc<SpillStore>,
}

impl<'a> Sources<'a> {
    pub fn new(read: &'a Read<'a>, measure: &'a Measure<'a>, store: &'a Arc<SpillStore>) -> Self {
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
    pub(super) placement: Level,
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
            Some(roi) => roi.contains(self.placement.voxel_center(voxel.map(|value| value as f32))),
            None => true,
        }
    }
}

pub struct SurfaceBuilder {
    level_index: usize,
    detection: usize,
    channels: usize,
    extent: [u32; 3],
    placement: Level,
    span: f32,
    intensity: Intensity,
    params: SurfaceParams,
    threshold: f32,
    region: [Range<u32>; 3],
    anchor: [i64; 3],
    tile: [u32; 3],
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
        let plane = u64::from(extent[0]) * u64::from(extent[1]);
        let slab = (SLAB_BUDGET / (8 * plane.max(1))).max(1) as u32;
        let tile_z = slab.clamp(TILE_Z_FLOOR, TILE_SIDE).div_ceil(TILE_Z_FLOOR) * TILE_Z_FLOOR;
        let tile = [TILE_SIDE, TILE_SIDE, tile_z];
        let tiles = [0, 1, 2].map(|axis| {
            let count = region[axis].end.saturating_sub(region[axis].start);
            count.div_ceil(tile[axis])
        });
        let tile_count = tiles[0] as usize * tiles[1] as usize * tiles[2] as usize;
        Self {
            level_index,
            detection,
            channels,
            extent,
            placement: level,
            span: intensity.span(),
            intensity,
            params,
            threshold: params.threshold(),
            region,
            anchor,
            tile,
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

    pub(super) fn placement(&self) -> Level {
        self.placement
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
            let start = self.region[axis].start + position[axis] * self.tile[axis];
            start..(start + self.tile[axis]).min(self.region[axis].end)
        })
    }

    pub(super) fn array_range(&self, index: usize, pad: [u32; 3]) -> [Range<u32>; 3] {
        let core = self.core_range(index);
        [0, 1, 2].map(|axis| {
            let start = core[axis].start.saturating_sub(pad[axis]);
            start..(core[axis].end + pad[axis]).min(self.extent[axis])
        })
    }

    pub(super) fn pad_morph(&self) -> [u32; 3] {
        [0; 3].map(|_| align(self.morph_radius + 1))
    }

    pub(super) fn coverage(&self, roi: Option<&Mask>) -> [Range<u32>; 3] {
        let bounds = roi.and_then(|roi| (!roi.is_empty()).then(|| roi.bounds()));
        [0, 1, 2].map(|axis| {
            let (mut start, mut end) = (self.region[axis].start, self.region[axis].end);
            if let Some(bounds) = &bounds {
                start = start.max(bounds[axis].start.clamp(0, i64::from(self.extent[axis])) as u32);
                end = end.min(bounds[axis].end.clamp(0, i64::from(self.extent[axis])) as u32);
            }
            if end <= start {
                start = self.region[axis].start;
                end = self.region[axis].end;
            }
            let reach = radius(self, axis);
            start.saturating_sub(reach)..end.saturating_add(reach).min(self.extent[axis])
        })
    }

    pub(super) fn field_range(&self, core: &[Range<u32>; 3]) -> [Range<u32>; 3] {
        [0, 1, 2].map(|axis| {
            let start = core[axis].start.saturating_sub(FIELD_REACH);
            let end = core[axis]
                .end
                .saturating_add(FIELD_REACH)
                .min(self.extent[axis]);
            start..end
        })
    }

    pub(super) fn gate<'a>(&'a self, roi: Option<&'a Mask>) -> Gate<'a> {
        Gate {
            region: [
                self.region[0].start,
                self.region[1].start,
                self.region[2].start,
            ],
            limit: [self.region[0].end, self.region[1].end, self.region[2].end],
            placement: self.placement,
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
        let centre = self
            .placement
            .voxel_of(seed)
            .map(|value| (value - 0.5).round() as i64);
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
        let total = self.tile_count;
        let step = total * 2 + 1;
        let cover = self.coverage(roi);
        let build = {
            let mut report = band(progress, step, 0, 35);
            DetectionStore::build(
                DetectionInput {
                    plan: &self,
                    cover: &cover,
                    roi,
                    workers: desired,
                },
                cancel,
                sources.read,
                DETECTION_CACHE,
                &mut report,
            )?
        };
        let Some(DetectionBuild {
            store: detection,
            field,
        }) = build
        else {
            return Ok(None);
        };
        let source = DetectionSource::new(&self, &cover, desired, cancel, &detection);
        let Some(mask) = mask::build(&source, roi, progress, step, &field)? else {
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
        let base = (DISTANCE_LIMIT * widest).min(DISTANCE_LIMIT_MAX);
        let distance = Distance::build(&self, &mask.solid, desired);
        let ceiling = base * 4;
        let margin = distance.margin().ceil() as u32 + 1;
        let limit = (distance.reach().ceil() as u32)
            .saturating_add(margin)
            .clamp(base, ceiling);
        let pass = Pass::new(&self, limit, seeds, &distance);
        let budget = Budget::new(FRAME_ARRAY_LIMIT);
        let workers = desired.min(pass.total().max(1));
        let topology = {
            let mut report = band(progress, step, 78, 100);
            let mut inputs = Inputs {
                cancel,
                progress: &mut report,
                measure: sources.measure,
                source: &source,
                solid: &mask.solid,
                field: mask.field,
                roi,
            };
            match run_pass(&pass, workers, &budget, &mut inputs)? {
                PassOutcome::Cancelled => return Ok(None),
                PassOutcome::Done(topology) => *topology,
            }
        };
        let topology = Geometry::paged(store, topology).map_err(|error| error.to_string())?;
        let loaded = topology.load().map_err(|error| error.to_string())?;
        let Some(set) = cut::cut(
            &loaded,
            self.params.cut(),
            &[],
            desired,
            Some(store),
            Some(cancel),
        )?
        else {
            return Ok(None);
        };
        progress(step, step);
        Ok(Some(
            set.with_topology(topology)
                .map_err(|error| error.to_string())?,
        ))
    }
}

pub fn surface_level(pyramid: &Pyramid, region: Region) -> Option<usize> {
    pyramid.fit_level(region, SURFACE_VOXELS)
}

pub fn recut(
    set: &SurfaceSet,
    spec: SurfaceCut,
    workers: usize,
    store: Option<&Arc<SpillStore>>,
) -> Result<Option<SurfaceSet>, String> {
    let Some(topology) = set.topology() else {
        return Ok(None);
    };
    let workers = crate::schedule::workers(workers);
    let loaded = topology.load().map_err(|error| error.to_string())?;
    let Some(cut) = cut::cut(&loaded, spec, set.removed(), workers, store, None)? else {
        return Ok(None);
    };
    let cut = cut
        .with_topology(topology.clone())
        .map_err(|error| error.to_string())?;
    Ok(Some(cut))
}

struct Pass<'a> {
    plan: &'a SurfaceBuilder,
    seeds: &'a [[i64; 3]],
    cap: u32,
    limit: u32,
    distance: &'a Distance,
}

impl<'a> Pass<'a> {
    fn new(
        plan: &'a SurfaceBuilder,
        limit: u32,
        seeds: &'a [[i64; 3]],
        distance: &'a Distance,
    ) -> Self {
        let cap = limit * limit;
        Self {
            plan,
            seeds,
            cap,
            limit,
            distance,
        }
    }

    fn total(&self) -> usize {
        self.plan.tile_count
    }

    fn pad(&self, index: usize) -> [u32; 3] {
        let plan = self.plan;
        let bound = self.distance.bound(&plan.core_range(index)).min(self.limit);
        let mut reach = bound;
        loop {
            let pad = [0, 1, 2].map(|axis| align(reach + radius(plan, axis) + 1));
            if array_bytes(plan, index, pad) <= WORKER_ARRAY_LIMIT {
                return pad;
            }
            assert!(
                reach > 0,
                "a surface tile fits its array budget at the smallest pad"
            );
            reach = reach.saturating_sub(PAD_ALIGN);
        }
    }

    fn array_bytes(&self, index: usize) -> u64 {
        array_bytes(self.plan, index, self.pad(index))
    }
}

fn radius(plan: &SurfaceBuilder, axis: usize) -> u32 {
    (plan.kernels[axis].len() / 2) as u32
}

fn array_bytes(plan: &SurfaceBuilder, index: usize, pad: [u32; 3]) -> u64 {
    let core = plan.core_range(index);
    let array = [0, 1, 2].map(|axis| {
        let size = (plan.region[axis].end - plan.region[axis].start) as u64;
        let span = u64::from(plan.tile[axis] + 2 * pad[axis]);
        size.min(span)
    });
    let field = plan
        .field_range(&core)
        .map(|range| u64::from(range.end - range.start));
    array.iter().product::<u64>() * ARRAY_BYTES_PER_VOXEL
        + field.iter().product::<u64>() * FIELD_BYTES_PER_VOXEL
}

enum PassOutcome {
    Done(Box<grana_object::SurfaceTopology>),
    Cancelled,
}

struct Inputs<'a> {
    cancel: &'a AtomicBool,
    progress: &'a mut dyn FnMut(usize, usize),
    measure: &'a Measure<'a>,
    source: &'a DetectionSource<'a>,
    solid: &'a SolidMask,
    field: &'a crate::local::BlockField,
    roi: Option<&'a Mask>,
}

fn run_pass(
    pass: &Pass<'_>,
    workers: usize,
    budget: &Budget,
    inputs: &mut Inputs<'_>,
) -> Result<PassOutcome, String> {
    if let Some(detection) = inputs.source.whole() {
        return run_whole(pass, workers, budget, &detection, inputs);
    }
    let total = pass.total();
    let per_row = pass.plan.tiles[0] as usize * pass.plan.tiles[1] as usize;
    let stop = AtomicBool::new(false);
    let cancel = inputs.cancel;
    let measure = inputs.measure;
    let solid = inputs.solid;
    let field = inputs.field;
    let gate = pass.plan.gate(inputs.roi);
    let mut assembler = Assembler::new(pass);
    let mut assembled = 0usize;
    for row in 0..pass.plan.tiles[2] as usize {
        let row_start = row * per_row;
        let row_end = (row_start + per_row).min(total);
        let core = pass.plan.core_range(row_start)[2].clone();
        let cover = inputs.source.cover();
        let z = [
            core.start.saturating_sub(FIELD_REACH).max(cover[2].start),
            core.end.saturating_add(FIELD_REACH).min(cover[2].end),
        ];
        let Some(detection) = inputs.source.slab(z[0]..z[1])? else {
            stop.store(true, Ordering::Relaxed);
            return Ok(PassOutcome::Cancelled);
        };
        if halted(cancel, &stop) {
            return Ok(PassOutcome::Cancelled);
        }
        let detection = &detection;
        let count = row_end - row_start;
        let (sender, receiver) = mpsc::channel::<(usize, TileResult)>();
        let next = AtomicUsize::new(0);
        let outcome = crate::schedule::parallel(|scope| {
            let next = &next;
            let stop = &stop;
            let gate = &gate;
            for _ in 0..workers.min(count.max(1)) {
                let sender = sender.clone();
                scope.spawn(move |_| {
                    let mut tile = Tile::new(pass, solid, field, detection, gate);
                    loop {
                        if halted(cancel, stop) {
                            break;
                        }
                        let local = next.fetch_add(1, Ordering::Relaxed);
                        if local >= count {
                            break;
                        }
                        let index = row_start + local;
                        let bytes = pass.array_bytes(index);
                        if !budget.acquire(bytes, || halted(cancel, stop)) {
                            break;
                        }
                        let result = tile.run(index, cancel, stop, measure);
                        budget.release(bytes);
                        if sender.send((index, result)).is_err() {
                            break;
                        }
                    }
                });
            }
            drop(sender);
            let mut pending: Vec<Option<TileResult>> = (0..count).map(|_| None).collect();
            let mut next_row = row_start;
            while next_row < row_end {
                if cancel.load(Ordering::Relaxed) {
                    stop.store(true, Ordering::Relaxed);
                    return Ok::<bool, String>(false);
                }
                let (index, result) = match crate::schedule::receive(&receiver) {
                    Some(value) => value,
                    None => {
                        if cancel.load(Ordering::Relaxed) {
                            return Ok(false);
                        }
                        return Err(
                            "the surface workers stopped before every tile was assembled"
                                .to_string(),
                        );
                    }
                };
                pending[index - row_start] = Some(result);
                while next_row < row_end {
                    let Some(result) = pending[next_row - row_start].take() else {
                        break;
                    };
                    match result {
                        TileResult::Cancelled => {
                            stop.store(true, Ordering::Relaxed);
                            return Ok(false);
                        }
                        TileResult::Failed(detail) => {
                            stop.store(true, Ordering::Relaxed);
                            return Err(detail);
                        }
                        TileResult::Finished(output) => {
                            assembler.push(next_row, *output)?;
                            next_row += 1;
                            assembled += 1;
                            if (assembled - 1).is_multiple_of(4) || assembled == total {
                                (inputs.progress)(total + assembled, total * 2 + 1);
                            }
                        }
                    }
                }
            }
            Ok(true)
        })?;
        if !outcome {
            return Ok(PassOutcome::Cancelled);
        }
    }
    Ok(PassOutcome::Done(Box::new(assembler.topology())))
}

fn run_whole(
    pass: &Pass<'_>,
    workers: usize,
    budget: &Budget,
    detection: &Detection,
    inputs: &mut Inputs<'_>,
) -> Result<PassOutcome, String> {
    let total = pass.total();
    let workers = workers.min(total.max(1));
    let next = AtomicUsize::new(0);
    let stop = AtomicBool::new(false);
    let cancel = inputs.cancel;
    let measure = inputs.measure;
    let solid = inputs.solid;
    let field = inputs.field;
    let gate = pass.plan.gate(inputs.roi);
    let (sender, receiver) = mpsc::channel::<(usize, TileResult)>();
    let outcome = crate::schedule::parallel(|scope| {
        let next = &next;
        let stop = &stop;
        let gate = &gate;
        for _ in 0..workers {
            let sender = sender.clone();
            scope.spawn(move |_| {
                let mut tile = Tile::new(pass, solid, field, detection, gate);
                loop {
                    if halted(cancel, stop) {
                        break;
                    }
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    if index >= total {
                        break;
                    }
                    let bytes = pass.array_bytes(index);
                    if !budget.acquire(bytes, || halted(cancel, stop)) {
                        break;
                    }
                    let result = tile.run(index, cancel, stop, measure);
                    budget.release(bytes);
                    if sender.send((index, result)).is_err() {
                        break;
                    }
                }
            });
        }
        drop(sender);
        let mut assembler = Assembler::new(pass);
        let mut pending: Vec<Option<TileResult>> = (0..total).map(|_| None).collect();
        let mut assembled = 0usize;
        while assembled < total {
            if cancel.load(Ordering::Relaxed) {
                stop.store(true, Ordering::Relaxed);
                return Ok(None);
            }
            let (index, result) = match crate::schedule::receive(&receiver) {
                Some(value) => value,
                None => {
                    if cancel.load(Ordering::Relaxed) {
                        return Ok(None);
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
                        return Ok(None);
                    }
                    TileResult::Failed(detail) => {
                        stop.store(true, Ordering::Relaxed);
                        return Err(detail);
                    }
                    TileResult::Finished(output) => {
                        assembler.push(assembled, *output)?;
                        assembled += 1;
                        if (assembled - 1).is_multiple_of(4) || assembled == total {
                            (inputs.progress)(total + assembled, total * 2 + 1);
                        }
                    }
                }
            }
        }
        Ok(Some(Box::new(assembler.topology())))
    })?;
    Ok(match outcome {
        Some(topology) => PassOutcome::Done(topology),
        None => PassOutcome::Cancelled,
    })
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

pub(super) fn band<'a>(
    report: &'a mut dyn FnMut(usize, usize),
    step: usize,
    from: usize,
    to: usize,
) -> impl FnMut(usize, usize) + 'a {
    let first = step * from / 100;
    let span = (step * to / 100).saturating_sub(first);
    move |done, units| {
        if let Some(share) = (done * span).checked_div(units.max(1)) {
            report(first + share, step);
        }
    }
}

pub(crate) struct Envelope {
    locations: Vec<usize>,
    boundaries: Vec<f64>,
}
impl Envelope {
    pub(crate) fn new() -> Self {
        Self {
            locations: Vec::new(),
            boundaries: Vec::new(),
        }
    }
}

fn parabola(line: &mut [f64], weight: f64, envelope: &mut Envelope) {
    let length = line.len();
    if length < 3 {
        return;
    }
    let Envelope {
        locations,
        boundaries,
    } = envelope;
    locations.clear();
    locations.resize(length, 0);
    boundaries.clear();
    boundaries.resize(length + 1, 0.0);
    let mut size = 0usize;
    boundaries[0] = f64::NEG_INFINITY;
    boundaries[1] = f64::INFINITY;
    for position in 1..length {
        let mut boundary = crossing(line, locations, size, position, weight);
        while size > 0 && boundary <= boundaries[size] {
            size -= 1;
            boundary = crossing(line, locations, size, position, weight);
        }
        size += 1;
        locations[size] = position;
        boundaries[size] = boundary;
        boundaries[size + 1] = f64::INFINITY;
    }
    let mut slot = 0usize;
    for position in 0..length {
        while boundaries[slot + 1] < position as f64 {
            slot += 1;
        }
        let distance = position as f64 - locations[slot] as f64;
        line[position] = weight * distance * distance + line[locations[slot]];
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
