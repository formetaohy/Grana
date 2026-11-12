mod operator;

use std::collections::HashMap;
use std::ops::Range;
use std::sync::{Arc, Mutex};

use grana_image::{Format, Image, Labels, Metric};
use grana_object::Mask;
use grana_optics::{KernelBank, Psf};
use grana_restore::{Job as RestoreJob, Restorer};
use grana_scene::{DerivedSpec, MaskChannel, Operation, Registration, Scene};
use grana_volume::{
    BrickCoord, BrickData, BrickGrid, Extent, Intensity, Level, Pyramid, VoxelData, pad_brick,
};
use grana_warp::Warp;

use operator::RegionSource;

const TILE: u32 = 64;
const TILE_LIMIT: usize = 16;
const TILE_BATCH: usize = 6;
const HALO_GROWTH_LIMIT: u32 = 32;
const BLEACH_WINDOW: u32 = 32;
const BLEACH_PLANES: u32 = 4;
const BLEACH_FRAMES: u32 = 32;
const BLEACH_PERCENTILE: usize = 99;

#[derive(Debug, thiserror::Error)]
pub enum ChannelsError {
    #[error(transparent)]
    Image(#[from] grana_image::Error),
    #[error("the derived channel cannot be read: {detail}")]
    Derived { detail: String },
}

impl From<ChannelsError> for String {
    fn from(error: ChannelsError) -> Self {
        error.to_string()
    }
}

enum Kind {
    Mask(Arc<MaskChannel>),
    Restore(RestoreChannel),
    Operator,
    Bleach {
        source: usize,
        factors: Result<Arc<[f32]>, String>,
    },
}

struct Node {
    spec: DerivedSpec,
    kind: Kind,
}

struct Site<'a> {
    source: &'a dyn RegionSource,
    time: u32,
    level: usize,
    info: Level,
    warp: &'a Warp,
}

struct RestoredTile {
    key: (u32, u32, u64, [u32; 3]),
    core: [Range<u32>; 3],
    ranges: [Range<u32>; 3],
    values: Vec<f32>,
}

impl RestoredTile {
    fn axis_weight(&self, axis: usize, position: u32) -> f32 {
        let core = &self.core[axis];
        let cover = &self.ranges[axis];
        let centre = position as f32 + 0.5;
        if position < core.start {
            ramp(centre, cover.start as f32, core.start as f32)
        } else if position >= core.end {
            ramp(centre, cover.end as f32, core.end as f32)
        } else {
            1.0
        }
    }
}

fn ramp(value: f32, edge: f32, inner: f32) -> f32 {
    let span = inner - edge;
    if span <= 0.0 {
        return 1.0;
    }
    let t = ((value - edge) / span).clamp(0.0, 1.0);
    0.5 - 0.5 * (std::f32::consts::PI * t).cos()
}

struct RestoreChannel {
    source: usize,
    psf: Psf,
    iterations: u32,
    registration: u64,
    banks: Mutex<HashMap<u32, Arc<KernelBank>>>,
    tiles: Mutex<Vec<Arc<RestoredTile>>>,
}

pub struct Channels {
    reader: Arc<Image>,
    derived: Vec<Node>,
    pyramid: Pyramid,
    metric: Option<Metric>,
    registration: Registration,
}

impl Channels {
    pub fn analysis(reader: Arc<Image>, scene: &Scene) -> Self {
        let metric = Metric::of(scene.dataset.pyramid());
        let pyramid = metric.as_ref().map_or_else(
            || scene.dataset.pyramid().clone(),
            |metric| metric.pyramid().clone(),
        );
        let mut channels = Self {
            pyramid,
            reader,
            derived: Vec::new(),
            metric,
            registration: scene.registration().clone(),
        };
        channels.derived = channels.derive(scene);
        channels
    }

    pub fn render(reader: Arc<Image>, scene: &Scene) -> Self {
        let mut channels = Self {
            pyramid: scene.dataset.pyramid().clone(),
            reader,
            derived: Vec::new(),
            metric: None,
            registration: scene.registration().clone(),
        };
        channels.derived = channels.derive(scene);
        channels
    }

    pub fn stored(reader: Arc<Image>) -> Self {
        let pyramid = reader.pyramid().clone();
        Self {
            pyramid,
            reader,
            derived: Vec::new(),
            metric: None,
            registration: Registration::default(),
        }
    }

    pub fn image(&self) -> &Image {
        &self.reader
    }

    pub fn image_shared(&self) -> Arc<Image> {
        Arc::clone(&self.reader)
    }

    pub fn pyramid(&self) -> &Pyramid {
        &self.pyramid
    }

    pub fn name(&self) -> &str {
        self.reader.name()
    }

    pub fn format(&self) -> Option<Format> {
        self.reader.format()
    }

    pub fn labels(&self) -> Result<Labels, grana_image::Error> {
        self.reader.labels()
    }

    pub fn restored_range(
        &self,
        source: usize,
        psf: Psf,
        iterations: u32,
    ) -> Result<Intensity, ChannelsError> {
        let stored = self.stored_channels();
        if source >= stored {
            return Err(ChannelsError::Derived {
                detail: "a restored channel restores a stored channel".to_string(),
            });
        }
        let info = self.level(0)?;
        let extent = info.extent();
        let limits = [extent.x(), extent.y(), extent.z()];
        let ranges = [0, 1, 2].map(|axis| {
            let window = limits[axis].min(TILE);
            let start = (limits[axis] - window) / 2;
            start..start + window
        });
        let restore = RestoreChannel::new(source, psf, iterations, self.registration.digest());
        let reader = ChannelSource {
            channels: self,
            time: 0,
            level: 0,
            warp: &Warp::IDENTITY,
        };
        let site = Site {
            source: &reader,
            time: 0,
            level: 0,
            info,
            warp: &Warp::IDENTITY,
        };
        let data = restore.region(&site, &ranges)?;
        let range = data.range();
        let fundus = self.reader.pyramid().channels()[source].intensity();
        let min = range.min().min(fundus.min());
        let max = (range.max() * 1.35).max(fundus.max());
        if !(min.is_finite() && max.is_finite() && min < max) {
            return Err(ChannelsError::Derived {
                detail: "the restored channel carries no intensity range".to_string(),
            });
        }
        Ok(Intensity::new(min, max))
    }

    pub fn ratio_range(
        &self,
        numerator: usize,
        denominator: usize,
    ) -> Result<Intensity, ChannelsError> {
        let channels = self.channels();
        if numerator >= channels || denominator >= channels || numerator == denominator {
            return Err(ChannelsError::Derived {
                detail: "a ratio channel divides two distinct channels of the dataset".to_string(),
            });
        }
        let info = self.level(0)?;
        let extent = info.extent();
        let limits = [extent.x(), extent.y(), extent.z()];
        let ranges = [0, 1, 2].map(|axis| {
            let window = limits[axis].min(TILE);
            let start = (limits[axis] - window) / 2;
            start..start + window
        });
        let up = self.read_region(0, 0, numerator, &ranges, &Warp::IDENTITY)?;
        let down = self.read_region(0, 0, denominator, &ranges, &Warp::IDENTITY)?;
        let mut up_values = vec![0.0f32; up.voxel_count()];
        up.write_values(&mut up_values);
        let mut down_values = vec![0.0f32; down.voxel_count()];
        down.write_values(&mut down_values);
        let mut max = 0.0f32;
        for (up, down) in up_values.iter().zip(&down_values) {
            if *down > 0.0 {
                max = max.max(up / down);
            }
        }
        Ok(Intensity::new(0.0, if max > 0.0 { max } else { 1.0 }))
    }

    fn channels(&self) -> usize {
        self.stored_channels() + self.derived.len()
    }

    fn stored_channels(&self) -> usize {
        self.reader.pyramid().channel_count()
    }

    fn level(&self, level: usize) -> Result<Level, ChannelsError> {
        self.pyramid
            .levels()
            .get(level)
            .copied()
            .ok_or_else(|| ChannelsError::Derived {
                detail: format!("the derived channel has no resolution level {level}"),
            })
    }

    pub fn read_region(
        &self,
        time: u32,
        level: usize,
        channel: usize,
        ranges: &[Range<u32>; 3],
        warp: &Warp,
    ) -> Result<VoxelData, ChannelsError> {
        let stored = self.stored_channels();
        if channel < stored {
            let warp = self.aligned(channel, warp);
            return self.read_stored(time, level, channel, ranges, &warp);
        }
        let node = self
            .derived
            .get(channel - stored)
            .ok_or_else(|| ChannelsError::Derived {
                detail: format!(
                    "the dataset holds {stored} stored channels and no channel {channel}"
                ),
            })?;
        let info = self.level(level)?;
        let reader = ChannelSource {
            channels: self,
            time,
            level,
            warp,
        };
        match &node.kind {
            Kind::Mask(mask_channel) => {
                let data = reader.read(mask_channel.source(), ranges)?;
                let mask = mask_channel
                    .mask(time, info)
                    .map_err(|detail| ChannelsError::Derived { detail })?;
                Ok(masked(&data, ranges, &mask))
            }
            Kind::Restore(restore) => restore.region(
                &Site {
                    source: &reader,
                    time,
                    level,
                    info,
                    warp,
                },
                ranges,
            ),
            Kind::Operator => operator::read(&node.spec, &reader, ranges, info),
            Kind::Bleach { source, factors } => {
                let factors = factors.as_ref().map_err(|detail| ChannelsError::Derived {
                    detail: detail.clone(),
                })?;
                let data = reader.read(*source, ranges)?;
                let factor = factors
                    .get(time as usize)
                    .copied()
                    .unwrap_or(1.0)
                    .clamp(1.0e-3, 1.0e3);
                Ok(scaled(&data, 1.0 / factor))
            }
        }
    }

    pub fn read_brick(
        &self,
        time: u32,
        level: usize,
        coord: BrickCoord,
        warp: &Warp,
    ) -> Result<BrickData, ChannelsError> {
        assert!(
            self.metric.is_none(),
            "a brick belongs to the source grid the renderer streams"
        );
        let data = match self.registration.is_empty() {
            true => self
                .reader
                .read_brick(time, level, coord, warp)
                .map_err(ChannelsError::Image)?,
            false => self.aligned_brick(time, level, coord, warp)?,
        };
        if self.derived.is_empty() {
            return Ok(data);
        }
        let info = self.level(level)?;
        let grid = BrickGrid::new(info.extent());
        let stored = self.stored_channels();
        let mut channels = data.channels().to_vec();
        for index in 0..self.derived.len() {
            let ranges = grid.padded_range(coord);
            let region = self.read_region(time, level, stored + index, &ranges, warp)?;
            let origin = [ranges[0].start, ranges[1].start, ranges[2].start];
            let covered = Extent::new(
                ranges[0].end - ranges[0].start,
                ranges[1].end - ranges[1].start,
                ranges[2].end - ranges[2].start,
            );
            channels.push(pad_brick(
                &region,
                origin,
                covered,
                grid.origin(coord),
                info.extent(),
            ));
        }
        Ok(BrickData::new(channels))
    }

    pub fn stored_region(
        &self,
        time: u32,
        level: usize,
        channel: usize,
        ranges: &[Range<u32>; 3],
        warp: &Warp,
    ) -> Result<VoxelData, ChannelsError> {
        assert!(
            channel < self.stored_channels(),
            "a stored read names a stored channel"
        );
        self.read_stored(time, level, channel, ranges, warp)
    }

    fn aligned(&self, channel: usize, warp: &Warp) -> Warp {
        warp.added(self.registration.warp(channel))
    }

    fn read_stored(
        &self,
        time: u32,
        level: usize,
        channel: usize,
        ranges: &[Range<u32>; 3],
        warp: &Warp,
    ) -> Result<VoxelData, ChannelsError> {
        match &self.metric {
            Some(metric) => metric
                .read_region(&self.reader, time, level, channel, ranges, warp)
                .map_err(ChannelsError::Image),
            None => self
                .reader
                .read_region(time, level, channel, ranges, warp)
                .map_err(ChannelsError::Image),
        }
    }

    fn aligned_brick(
        &self,
        time: u32,
        level: usize,
        coord: BrickCoord,
        warp: &Warp,
    ) -> Result<BrickData, ChannelsError> {
        let info = self.level(level)?;
        let grid = BrickGrid::new(info.extent());
        let ranges = grid.padded_range(coord);
        let origin = [ranges[0].start, ranges[1].start, ranges[2].start];
        let covered = Extent::new(
            ranges[0].end - ranges[0].start,
            ranges[1].end - ranges[1].start,
            ranges[2].end - ranges[2].start,
        );
        let mut channels = Vec::with_capacity(self.stored_channels());
        for channel in 0..self.stored_channels() {
            let warp = self.aligned(channel, warp);
            let region = self.read_stored(time, level, channel, &ranges, &warp)?;
            channels.push(pad_brick(
                &region,
                origin,
                covered,
                grid.origin(coord),
                info.extent(),
            ));
        }
        Ok(BrickData::new(channels))
    }
}

impl Channels {
    fn derive(&self, scene: &Scene) -> Vec<Node> {
        scene
            .derived()
            .iter()
            .map(|entry| {
                let spec = entry.spec().clone();
                let kind = match spec.operation() {
                    Operation::Mask { .. } => Kind::Mask(
                        entry
                            .mask()
                            .expect("a bound mask channel carries its object frames")
                            .clone(),
                    ),
                    Operation::Restore(restore) => Kind::Restore(RestoreChannel::new(
                        spec.source(),
                        restore.psf().clone(),
                        restore.iterations(),
                        self.registration.digest(),
                    )),
                    Operation::Bleach => Kind::Bleach {
                        source: spec.source(),
                        factors: self
                            .bleach_factors(spec.source(), scene.drift())
                            .map_err(|error| error.to_string()),
                    },
                    Operation::Smooth(_) | Operation::Background(_) | Operation::Ratio(_) => {
                        Kind::Operator
                    }
                };
                Node { spec, kind }
            })
            .collect()
    }

    pub fn bleach_factors(
        &self,
        source: usize,
        drift: &grana_scene::Drift,
    ) -> Result<Arc<[f32]>, ChannelsError> {
        let stored = self.stored_channels();
        if source >= stored {
            return Err(ChannelsError::Derived {
                detail: "a bleach correction normalizes a stored channel".to_string(),
            });
        }
        let frames = self.pyramid.frame_count();
        let level = self.bleach_level();
        let info = self.level(level)?;
        let extent = info.extent();
        let ranges = [0, 1, 2].map(|axis| {
            let limit = [extent.x(), extent.y(), extent.z()][axis];
            let span = limit.min(BLEACH_WINDOW).min(if axis == 2 {
                BLEACH_PLANES
            } else {
                BLEACH_WINDOW
            });
            let start = (limit - span) / 2;
            start..start + span
        });
        let sampled = frames.clamp(1, BLEACH_FRAMES);
        let mut levels = Vec::with_capacity(sampled as usize);
        for index in 0..sampled {
            let time = (index as u64 * u64::from(frames - 1)
                / u64::from(sampled.saturating_sub(1).max(1))) as u32;
            let data = self
                .reader
                .read_region(
                    time.min(frames - 1),
                    level,
                    source,
                    &ranges,
                    drift.warp(time),
                )
                .map_err(ChannelsError::Image)?;
            let mut values = vec![0.0f32; data.voxel_count()];
            data.write_values(&mut values);
            values.retain(|value| value.is_finite());
            values.sort_unstable_by(f32::total_cmp);
            let bright = values
                .get((values.len().saturating_sub(1)) * BLEACH_PERCENTILE / 100)
                .copied()
                .unwrap_or(0.0);
            levels.push(bright);
        }
        let reference = levels.first().copied().unwrap_or(0.0);
        let factors = (0..frames)
            .map(|time| {
                if frames <= 1 {
                    return 1.0;
                }
                let position = time as f32 * (sampled - 1) as f32 / (frames - 1) as f32;
                let low = (position.floor() as u32).min(sampled - 1) as usize;
                let high = (low + 1).min(sampled as usize - 1);
                let weight = position - low as f32;
                let bright = levels[low] * (1.0 - weight) + levels[high] * weight;
                if reference > 0.0 && bright > 0.0 {
                    (bright / reference).clamp(1.0e-3, 1.0e3)
                } else {
                    1.0
                }
            })
            .collect::<Vec<_>>();
        Ok(factors.into())
    }

    pub fn bleach_range(
        &self,
        source: usize,
        drift: &grana_scene::Drift,
    ) -> Result<Intensity, ChannelsError> {
        let factors = self.bleach_factors(source, drift)?;
        let low = factors.iter().copied().fold(f32::INFINITY, f32::min);
        let high = factors.iter().copied().fold(0.0f32, f32::max);
        let fundus = self.reader.pyramid().channels()[source].intensity();
        if !(low.is_finite() && high.is_finite() && low > 0.0 && high > 0.0) {
            return Ok(fundus);
        }
        let min = fundus.min() / high;
        let max = fundus.max() / low;
        if min.is_finite() && max.is_finite() && min < max {
            Ok(Intensity::new(min, max))
        } else {
            Ok(fundus)
        }
    }

    fn bleach_level(&self) -> usize {
        let mut level = 0;
        for index in 0..self.pyramid.level_count() {
            if self.pyramid.level(index).extent().max_dimension() >= BLEACH_WINDOW {
                level = index;
            }
        }
        level.min(self.pyramid.stored().saturating_sub(1))
    }
}

struct ChannelSource<'a> {
    channels: &'a Channels,
    time: u32,
    level: usize,
    warp: &'a Warp,
}

impl RegionSource for ChannelSource<'_> {
    fn read(&self, channel: usize, ranges: &[Range<u32>; 3]) -> Result<VoxelData, ChannelsError> {
        self.channels
            .read_region(self.time, self.level, channel, ranges, self.warp)
    }
}

impl RestoreChannel {
    fn new(source: usize, psf: Psf, iterations: u32, registration: u64) -> Self {
        Self {
            source,
            psf,
            iterations,
            registration,
            banks: Mutex::new(HashMap::new()),
            tiles: Mutex::new(Vec::new()),
        }
    }

    fn warp_digest(&self, warp: &Warp) -> u64 {
        let mut digest = warp.digest();
        digest = digest.rotate_left(17) ^ self.registration;
        digest.wrapping_mul(0x0000_0100_0000_01b3)
    }

    fn bank(&self, site: &Site<'_>) -> Arc<KernelBank> {
        let key = site.level as u32;
        let mut banks = self.banks.lock().unwrap_or_else(|error| error.into_inner());
        if let Some(bank) = banks.get(&key) {
            return Arc::clone(bank);
        }
        let bank = Arc::new(self.psf.bank(site.info.spacing(), site.info.extent().z()));
        banks.insert(key, Arc::clone(&bank));
        bank
    }

    fn region(
        &self,
        site: &Site<'_>,
        ranges: &[Range<u32>; 3],
    ) -> Result<VoxelData, ChannelsError> {
        let limits = {
            let extent = site.info.extent();
            [extent.x(), extent.y(), extent.z()]
        };
        if (0..3)
            .any(|axis| ranges[axis].start >= ranges[axis].end || ranges[axis].end > limits[axis])
        {
            return Err(ChannelsError::Derived {
                detail: "the requested region lies outside the volume".to_string(),
            });
        }
        let bank = self.bank(site);
        let dims = [0, 1, 2].map(|axis| ranges[axis].end - ranges[axis].start);
        let width = dims[0] as usize;
        let height = dims[1] as usize;
        let voxels = (width * height * dims[2] as usize).max(1);
        let mut sum = Sum {
            values: vec![0.0f32; voxels],
            weights: vec![0.0f32; voxels],
            ranges: ranges.clone(),
            width,
            height,
        };
        let halo = halo(bank.dims());
        let counts = [0, 1, 2].map(|axis| limits[axis].div_ceil(TILE));
        let first = [0, 1, 2].map(|axis| ranges[axis].start.saturating_sub(halo[axis]) / TILE);
        let last = [0, 1, 2].map(|axis| {
            let end = (ranges[axis].end + halo[axis]).min(limits[axis]);
            ((end - 1) / TILE).min(counts[axis] - 1)
        });
        let mut pending = Vec::with_capacity(TILE_BATCH);
        for tile_z in first[2]..=last[2] {
            for tile_y in first[1]..=last[1] {
                for tile_x in first[0]..=last[0] {
                    let coord = [tile_x, tile_y, tile_z];
                    match self.held(site, coord) {
                        Some(tile) => sum.add(&tile),
                        None => {
                            pending.push(self.plan(site, &bank, coord));
                            if pending.len() == TILE_BATCH {
                                self.flush(site, &bank, &mut pending, &mut sum)?;
                            }
                        }
                    }
                }
            }
        }
        if !pending.is_empty() {
            self.flush(site, &bank, &mut pending, &mut sum)?;
        }
        for (value, weight) in sum.values.iter_mut().zip(&sum.weights) {
            assert!(*weight > 0.0, "every restored voxel lies under a tile");
            *value /= *weight;
        }
        Ok(VoxelData::F32(sum.values))
    }

    fn held(&self, site: &Site<'_>, coord: [u32; 3]) -> Option<Arc<RestoredTile>> {
        let key = (
            site.time,
            site.level as u32,
            self.warp_digest(site.warp),
            coord,
        );
        let mut tiles = self.tiles.lock().unwrap_or_else(|error| error.into_inner());
        let index = tiles.iter().position(|tile| tile.key == key)?;
        let tile = Arc::clone(&tiles[index]);
        tiles.remove(index);
        tiles.push(Arc::clone(&tile));
        Some(tile)
    }

    fn plan(&self, site: &Site<'_>, bank: &KernelBank, coord: [u32; 3]) -> TilePlan {
        let limits = {
            let extent = site.info.extent();
            [extent.x(), extent.y(), extent.z()]
        };
        let core = [0, 1, 2].map(|axis| {
            let start = coord[axis] * TILE;
            start..(start + TILE).min(limits[axis])
        });
        assert!(
            core.iter().all(|range| range.start < range.end),
            "a restored tile lies inside the volume"
        );
        let halo = halo(bank.dims());
        let growth = self.iterations.min(HALO_GROWTH_LIMIT);
        let ranges = expand(&core, &halo, &limits);
        let probe = expand(&ranges, &[0, 1, 2].map(|axis| halo[axis] + growth), &limits);
        TilePlan {
            key: (
                site.time,
                site.level as u32,
                self.warp_digest(site.warp),
                coord,
            ),
            core,
            ranges,
            probe,
        }
    }

    fn flush(
        &self,
        site: &Site<'_>,
        bank: &KernelBank,
        pending: &mut Vec<TilePlan>,
        sum: &mut Sum,
    ) -> Result<(), ChannelsError> {
        let plans = std::mem::take(pending);
        let mut sources = Vec::with_capacity(plans.len());
        for plan in &plans {
            let extent = extent_of(&plan.probe);
            let data = site.source.read(self.source, &plan.probe)?;
            let mut values = vec![0.0f32; extent.voxel_count() as usize];
            data.write_values(&mut values);
            sources.push(values);
        }
        let jobs = plans
            .iter()
            .zip(&sources)
            .map(|(plan, source)| RestoreJob {
                dims: [
                    plan.probe[0].end - plan.probe[0].start,
                    plan.probe[1].end - plan.probe[1].start,
                    plan.probe[2].end - plan.probe[2].start,
                ],
                source,
                first_plane: plan.probe[2].start,
                iterations: self.iterations,
            })
            .collect::<Vec<_>>();
        let restored = Restorer::shared().restore_batch(bank, &jobs);
        let mut tiles = self.tiles.lock().unwrap_or_else(|error| error.into_inner());
        for (plan, values) in plans.iter().zip(&restored) {
            let covered = extent_of(&plan.ranges);
            let mut tile_values = vec![0.0f32; covered.voxel_count() as usize];
            copy(
                &mut Target {
                    values: &mut tile_values,
                    extent: covered,
                    ranges: plan.ranges.clone(),
                },
                &Source {
                    values,
                    extent: extent_of(&plan.probe),
                    ranges: plan.probe.clone(),
                },
                &plan.ranges,
            );
            let tile = Arc::new(RestoredTile {
                key: plan.key,
                core: plan.core.clone(),
                ranges: plan.ranges.clone(),
                values: tile_values,
            });
            tiles.push(Arc::clone(&tile));
            if tiles.len() > TILE_LIMIT {
                tiles.remove(0);
            }
            sum.add(&tile);
        }
        Ok(())
    }
}

struct TilePlan {
    key: (u32, u32, u64, [u32; 3]),
    core: [Range<u32>; 3],
    ranges: [Range<u32>; 3],
    probe: [Range<u32>; 3],
}

struct Sum {
    values: Vec<f32>,
    weights: Vec<f32>,
    ranges: [Range<u32>; 3],
    width: usize,
    height: usize,
}

impl Sum {
    fn add(&mut self, tile: &RestoredTile) {
        let overlap = intersection(&self.ranges, &tile.ranges);
        if overlap.iter().any(|range| range.start >= range.end) {
            return;
        }
        let axis = [0, 1, 2].map(|axis| {
            overlap[axis]
                .clone()
                .map(|position| tile.axis_weight(axis, position))
                .collect::<Vec<f32>>()
        });
        let tile_width = (tile.ranges[0].end - tile.ranges[0].start) as usize;
        let tile_height = (tile.ranges[1].end - tile.ranges[1].start) as usize;
        for z in overlap[2].clone() {
            for y in overlap[1].clone() {
                for x in overlap[0].clone() {
                    let weight = axis[0][(x - overlap[0].start) as usize]
                        * axis[1][(y - overlap[1].start) as usize]
                        * axis[2][(z - overlap[2].start) as usize];
                    let target = (x - self.ranges[0].start) as usize
                        + self.width
                            * ((y - self.ranges[1].start) as usize
                                + self.height * (z - self.ranges[2].start) as usize);
                    let source = (x - tile.ranges[0].start) as usize
                        + tile_width
                            * ((y - tile.ranges[1].start) as usize
                                + tile_height * (z - tile.ranges[2].start) as usize);
                    self.values[target] += weight * tile.values[source];
                    self.weights[target] += weight;
                }
            }
        }
    }
}

fn scaled(data: &VoxelData, factor: f32) -> VoxelData {
    let mut values = vec![0.0f32; data.voxel_count()];
    data.write_values(&mut values);
    for value in &mut values {
        *value *= factor;
    }
    VoxelData::F32(values)
}

fn halo(dims: [u32; 3]) -> [u32; 3] {
    [dims[0] / 2, dims[1] / 2, dims[2] / 2]
}

fn expand(ranges: &[Range<u32>; 3], by: &[u32; 3], limits: &[u32; 3]) -> [Range<u32>; 3] {
    [0, 1, 2].map(|axis| {
        ranges[axis].start.saturating_sub(by[axis])..(ranges[axis].end + by[axis]).min(limits[axis])
    })
}

fn extent_of(ranges: &[Range<u32>; 3]) -> Extent {
    Extent::new(
        ranges[0].end - ranges[0].start,
        ranges[1].end - ranges[1].start,
        ranges[2].end - ranges[2].start,
    )
}

fn intersection(left: &[Range<u32>; 3], right: &[Range<u32>; 3]) -> [Range<u32>; 3] {
    [0, 1, 2]
        .map(|axis| left[axis].start.max(right[axis].start)..left[axis].end.min(right[axis].end))
}

struct Target<'a> {
    values: &'a mut [f32],
    extent: Extent,
    ranges: [Range<u32>; 3],
}

struct Source<'a> {
    values: &'a [f32],
    extent: Extent,
    ranges: [Range<u32>; 3],
}

fn copy(target: &mut Target<'_>, source: &Source<'_>, overlap: &[Range<u32>; 3]) {
    let width = (overlap[0].end - overlap[0].start) as usize;
    for z in overlap[2].clone() {
        for y in overlap[1].clone() {
            let source_start = source.extent.index(
                overlap[0].start - source.ranges[0].start,
                y - source.ranges[1].start,
                z - source.ranges[2].start,
            ) as usize;
            let target_start = target.extent.index(
                overlap[0].start - target.ranges[0].start,
                y - target.ranges[1].start,
                z - target.ranges[2].start,
            ) as usize;
            target.values[target_start..target_start + width]
                .copy_from_slice(&source.values[source_start..source_start + width]);
        }
    }
}

pub fn masked(data: &VoxelData, ranges: &[Range<u32>; 3], mask: &Mask) -> VoxelData {
    let width = (ranges[0].end - ranges[0].start).max(1);
    let height = (ranges[1].end - ranges[1].start).max(1);
    let depth = (ranges[2].end - ranges[2].start).max(1);
    match data {
        VoxelData::U8(values) => {
            VoxelData::U8(mask_values(values, ranges, width, height, depth, mask))
        }
        VoxelData::U16(values) => {
            VoxelData::U16(mask_values(values, ranges, width, height, depth, mask))
        }
        VoxelData::F32(values) => {
            VoxelData::F32(mask_values(values, ranges, width, height, depth, mask))
        }
    }
}

fn mask_values<T: Copy + Default>(
    values: &[T],
    ranges: &[Range<u32>; 3],
    width: u32,
    height: u32,
    depth: u32,
    mask: &Mask,
) -> Vec<T> {
    assert_eq!(
        values.len(),
        (width * height * depth) as usize,
        "the masked channel covers every voxel of the request"
    );
    let mut masked = values.to_vec();
    let mut index = 0;
    for z in 0..depth {
        for y in 0..height {
            for x in 0..width {
                let cell = [
                    ranges[0].start + x,
                    ranges[1].start + y,
                    ranges[2].start + z,
                ];
                if !mask.contains_cell(cell) {
                    masked[index] = T::default();
                }
                index += 1;
            }
        }
    }
    masked
}
