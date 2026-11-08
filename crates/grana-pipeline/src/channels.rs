use std::collections::HashMap;
use std::ops::Range;
use std::sync::{Arc, Mutex};

use grana_image::{Format, Image, Labels};
use grana_object::Mask;
use grana_optics::{KernelBank, Psf};
use grana_restore::{Job as RestoreJob, Restorer};
use grana_scene::{DerivedSpec, MaskChannel, Scene};
use grana_volume::{
    BrickCoord, BrickData, BrickGrid, Extent, Intensity, Level, PADDED_SIDE, Pyramid, VoxelData,
    pad_brick,
};

const TILE: u32 = 64;
const TILE_LIMIT: usize = 16;
const TILE_BATCH: usize = 6;
const HALO_GROWTH_LIMIT: u32 = 32;

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

enum Derived {
    Mask(Arc<MaskChannel>),
    Restore(RestoreChannel),
}

struct Site<'a> {
    reader: &'a Image,
    time: u32,
    level: usize,
    info: Level,
    shift: [f32; 3],
}

struct RestoredTile {
    key: (u32, u32, [u32; 3], [u32; 3]),
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
    banks: Mutex<HashMap<u32, Arc<KernelBank>>>,
    tiles: Mutex<Vec<Arc<RestoredTile>>>,
}

pub struct Channels {
    reader: Arc<Image>,
    derived: Vec<Derived>,
    pyramid: Pyramid,
}

impl Channels {
    pub fn new(reader: Arc<Image>, scene: &Scene) -> Self {
        let derived = scene
            .derived()
            .iter()
            .map(|entry| match entry.spec() {
                DerivedSpec::Mask(_) => Derived::Mask(
                    entry
                        .mask()
                        .expect("a bound mask channel carries its object frames")
                        .clone(),
                ),
                DerivedSpec::Restore(spec) => Derived::Restore(RestoreChannel::new(
                    spec.channel(),
                    spec.psf().clone(),
                    spec.iterations(),
                )),
            })
            .collect();
        Self {
            pyramid: scene.dataset.pyramid().clone(),
            reader,
            derived,
        }
    }

    pub fn stored(reader: Arc<Image>) -> Self {
        let pyramid = reader.pyramid().clone();
        Self {
            reader,
            derived: Vec::new(),
            pyramid,
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
        let restore = RestoreChannel::new(source, psf, iterations);
        let site = Site {
            reader: &self.reader,
            time: 0,
            level: 0,
            info,
            shift: [0.0; 3],
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
        shift: [f32; 3],
    ) -> Result<VoxelData, ChannelsError> {
        let stored = self.stored_channels();
        if channel < stored {
            return self
                .reader
                .read_region(time, level, channel, ranges, shift)
                .map_err(ChannelsError::Image);
        }
        let derived = self
            .derived
            .get(channel - stored)
            .ok_or_else(|| ChannelsError::Derived {
                detail: format!(
                    "the dataset holds {stored} stored channels and no channel {channel}"
                ),
            })?;
        let info = self.level(level)?;
        match derived {
            Derived::Mask(mask_channel) => {
                let data = self
                    .reader
                    .read_region(time, level, mask_channel.source(), ranges, shift)
                    .map_err(ChannelsError::Image)?;
                let mask = mask_channel
                    .mask(time, info)
                    .map_err(|detail| ChannelsError::Derived { detail })?;
                Ok(masked(&data, ranges, &mask))
            }
            Derived::Restore(restore) => restore.region(
                &Site {
                    reader: &self.reader,
                    time,
                    level,
                    info,
                    shift,
                },
                ranges,
            ),
        }
    }

    pub fn read_brick(
        &self,
        time: u32,
        level: usize,
        coord: BrickCoord,
        shift: [f32; 3],
    ) -> Result<BrickData, ChannelsError> {
        let data = self
            .reader
            .read_brick(time, level, coord, shift)
            .map_err(ChannelsError::Image)?;
        if self.derived.is_empty() {
            return Ok(data);
        }
        let info = self.level(level)?;
        let grid = BrickGrid::new(info.extent());
        let mut channels = data.channels().to_vec();
        for derived in &self.derived {
            channels.push(match derived {
                Derived::Mask(mask_channel) => {
                    let mask = mask_channel
                        .mask(time, info)
                        .map_err(|detail| ChannelsError::Derived { detail })?;
                    masked_brick(&data.channels()[mask_channel.source()], &mask, grid, coord)
                }
                Derived::Restore(restore) => restore.brick(
                    &Site {
                        reader: &self.reader,
                        time,
                        level,
                        info,
                        shift,
                    },
                    grid,
                    coord,
                )?,
            });
        }
        Ok(BrickData::new(channels))
    }
}

impl RestoreChannel {
    fn new(source: usize, psf: Psf, iterations: u32) -> Self {
        Self {
            source,
            psf,
            iterations,
            banks: Mutex::new(HashMap::new()),
            tiles: Mutex::new(Vec::new()),
        }
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

    fn brick(
        &self,
        site: &Site<'_>,
        grid: BrickGrid,
        coord: BrickCoord,
    ) -> Result<VoxelData, ChannelsError> {
        let spatial = grid.padded_range(coord);
        let origin = [spatial[0].start, spatial[1].start, spatial[2].start];
        let covered = Extent::new(
            spatial[0].end - spatial[0].start,
            spatial[1].end - spatial[1].start,
            spatial[2].end - spatial[2].start,
        );
        let data = self.region(site, &spatial)?;
        Ok(pad_brick(
            &data,
            origin,
            covered,
            grid.origin(coord),
            site.info.extent(),
        ))
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
            site.shift.map(f32::to_bits),
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
                site.shift.map(f32::to_bits),
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
            let data = site
                .reader
                .read_region(site.time, site.level, self.source, &plan.probe, site.shift)
                .map_err(ChannelsError::Image)?;
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
    key: (u32, u32, [u32; 3], [u32; 3]),
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

fn masked_brick(data: &VoxelData, mask: &Mask, grid: BrickGrid, coord: BrickCoord) -> VoxelData {
    match data {
        VoxelData::U8(values) => VoxelData::U8(mask_cube(values, mask, grid, coord)),
        VoxelData::U16(values) => VoxelData::U16(mask_cube(values, mask, grid, coord)),
        VoxelData::F32(values) => VoxelData::F32(mask_cube(values, mask, grid, coord)),
    }
}

fn mask_cube<T: Copy + Default>(
    values: &[T],
    mask: &Mask,
    grid: BrickGrid,
    coord: BrickCoord,
) -> Vec<T> {
    assert_eq!(
        values.len(),
        (PADDED_SIDE as usize).pow(3),
        "the masked cube holds every padded voxel"
    );
    let mut masked = values.to_vec();
    let mut index = 0;
    for z in 0..PADDED_SIDE {
        for y in 0..PADDED_SIDE {
            for x in 0..PADDED_SIDE {
                if !mask.contains_cell(grid.padded_cell(coord, [x, y, z])) {
                    masked[index] = T::default();
                }
                index += 1;
            }
        }
    }
    masked
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
