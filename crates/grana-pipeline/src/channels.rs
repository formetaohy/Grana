use std::collections::HashMap;
use std::ops::Range;
use std::sync::{Arc, Mutex};

use grana_image::{Format, Image, Labels};
use grana_object::Mask;
use grana_optics::{Kernel, PsfParams};
use grana_restore::Restorer;
use grana_scene::{DerivedSpec, MaskChannel, Scene};
use grana_volume::{
    BrickCoord, BrickData, BrickGrid, Extent, Intensity, Level, PADDED_SIDE, Pyramid, Spacing,
    VoxelData, pad_brick,
};

const TILE: u32 = 64;
const TILE_LIMIT: usize = 16;

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
    origin: [u32; 3],
    dims: [u32; 3],
    values: Vec<f32>,
}

struct RestoreChannel {
    source: usize,
    params: PsfParams,
    iterations: u32,
    kernels: Mutex<HashMap<u32, Arc<Kernel>>>,
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
                    spec.params(),
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
        params: PsfParams,
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
        let restore = RestoreChannel::new(source, params, iterations);
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
    fn new(source: usize, params: PsfParams, iterations: u32) -> Self {
        Self {
            source,
            params,
            iterations,
            kernels: Mutex::new(HashMap::new()),
            tiles: Mutex::new(Vec::new()),
        }
    }

    fn kernel(&self, level: usize, spacing: Spacing) -> Result<Arc<Kernel>, ChannelsError> {
        let key = level as u32;
        let mut kernels = self
            .kernels
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(kernel) = kernels.get(&key) {
            return Ok(Arc::clone(kernel));
        }
        let kernel = Arc::new(Kernel::new(self.params, spacing));
        kernels.insert(key, Arc::clone(&kernel));
        Ok(kernel)
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
        let info = site.info;
        let extent = info.extent();
        let limits = [extent.x(), extent.y(), extent.z()];
        let kernel = self.kernel(site.level, info.spacing())?;
        let dims = kernel.dims();
        let halo = [dims[0] / 2, dims[1] / 2, dims[2] / 2];
        let probe = [0, 1, 2].map(|axis| {
            let start = ranges[axis].start.saturating_sub(halo[axis]);
            let end = (ranges[axis].end + halo[axis]).min(limits[axis]);
            start..end
        });
        if probe.iter().any(|range| range.start >= range.end) {
            return Err(ChannelsError::Derived {
                detail: "the requested region lies outside the volume".to_string(),
            });
        }
        let probe_extent = extent_of(&probe);
        let mut probe_values = vec![0.0f32; probe_extent.voxel_count() as usize];
        let first = [0, 1, 2].map(|axis| probe[axis].start / TILE);
        let last = [0, 1, 2].map(|axis| (probe[axis].end - 1) / TILE);
        for z in first[2]..=last[2] {
            for y in first[1]..=last[1] {
                for x in first[0]..=last[0] {
                    let tile = self.tile(site, &kernel, [x, y, z])?;
                    let covered = [
                        tile.origin[0]..tile.origin[0] + tile.dims[0],
                        tile.origin[1]..tile.origin[1] + tile.dims[1],
                        tile.origin[2]..tile.origin[2] + tile.dims[2],
                    ];
                    let overlap = intersection(&probe, &covered);
                    if overlap.iter().any(|range| range.start >= range.end) {
                        continue;
                    }
                    copy(
                        &mut Target {
                            values: &mut probe_values,
                            extent: probe_extent,
                            ranges: probe.clone(),
                        },
                        &Source {
                            values: &tile.values,
                            extent: extent_of(&covered),
                            ranges: covered,
                        },
                        &overlap,
                    );
                }
            }
        }
        let mut output = vec![0.0f32; extent_of(ranges).voxel_count() as usize];
        copy(
            &mut Target {
                values: &mut output,
                extent: extent_of(ranges),
                ranges: ranges.clone(),
            },
            &Source {
                values: &probe_values,
                extent: probe_extent,
                ranges: probe,
            },
            ranges,
        );
        Ok(VoxelData::F32(output))
    }

    fn tile(
        &self,
        site: &Site<'_>,
        kernel: &Kernel,
        coord: [u32; 3],
    ) -> Result<Arc<RestoredTile>, ChannelsError> {
        let key = (
            site.time,
            site.level as u32,
            site.shift.map(f32::to_bits),
            coord,
        );
        let mut tiles = self.tiles.lock().unwrap_or_else(|error| error.into_inner());
        if let Some(index) = tiles.iter().position(|tile| tile.key == key) {
            let tile = Arc::clone(&tiles[index]);
            tiles.remove(index);
            tiles.push(Arc::clone(&tile));
            return Ok(tile);
        }
        let extent = site.info.extent();
        let limits = [extent.x(), extent.y(), extent.z()];
        let origin = [coord[0] * TILE, coord[1] * TILE, coord[2] * TILE];
        let dims = [0, 1, 2].map(|axis| (extent_dims(extent)[axis] - origin[axis]).min(TILE));
        let core = [0, 1, 2].map(|axis| origin[axis]..origin[axis] + dims[axis]);
        let dims_kernel = kernel.dims();
        let halo = [dims_kernel[0] / 2, dims_kernel[1] / 2, dims_kernel[2] / 2];
        let probe = [0, 1, 2].map(|axis| {
            let start = core[axis].start.saturating_sub(halo[axis]);
            let end = (core[axis].end + halo[axis]).min(limits[axis]);
            start..end
        });
        let probe_extent = extent_of(&probe);
        let source = site
            .reader
            .read_region(site.time, site.level, self.source, &probe, site.shift)
            .map_err(ChannelsError::Image)?;
        let mut values = vec![0.0f32; probe_extent.voxel_count() as usize];
        source.write_values(&mut values);
        let restored = Restorer::shared().restore(
            [probe_extent.x(), probe_extent.y(), probe_extent.z()],
            &values,
            kernel,
            self.iterations,
        );
        let mut core_values = vec![0.0f32; dims[0] as usize * dims[1] as usize * dims[2] as usize];
        copy(
            &mut Target {
                values: &mut core_values,
                extent: Extent::new(dims[0], dims[1], dims[2]),
                ranges: core.clone(),
            },
            &Source {
                values: &restored,
                extent: probe_extent,
                ranges: probe,
            },
            &core,
        );
        let tile = Arc::new(RestoredTile {
            key,
            origin,
            dims,
            values: core_values,
        });
        tiles.push(Arc::clone(&tile));
        if tiles.len() > TILE_LIMIT {
            tiles.remove(0);
        }
        Ok(tile)
    }
}

fn extent_dims(extent: Extent) -> [u32; 3] {
    [extent.x(), extent.y(), extent.z()]
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
