use std::fs::File;
use std::io::Read;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use grana_volume::{
    BrickCoord, BrickData, BrickGrid, Extent, Pyramid, VolumeSeries, VoxelData, VoxelFormat,
    pad_brick,
};

use crate::cache::{self, Key, Memory, Shape, Store};
use crate::level::{self, Field, Scale};
use crate::tile::{self, Grid};
use crate::{Error, Series, Source};

const RESOLVED_VERSION: u32 = 1;
const PARTIAL_DIVISOR: u64 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Zarr,
    Tiff,
    Czi,
}

enum Inner {
    Memory(Arc<VolumeSeries>),
    Zarr(grana_zarr::Image),
    Tiff(grana_tiff::Image),
    Czi(grana_czi::Image),
}

pub struct Image {
    inner: Inner,
    pyramid: Pyramid,
    series: Vec<Series>,
    selected: usize,
    identity: u64,
    tiles: Mutex<Memory>,
    store: Option<Store>,
}

impl Image {
    pub fn open(source: &Source) -> Result<Self, Error> {
        Self::open_with_cache(source, cache::default_root())
    }

    pub fn open_with_cache(source: &Source, cache: Option<PathBuf>) -> Result<Self, Error> {
        let (inner, series) = match source {
            Source::Memory(series) => {
                let inner = Inner::Memory(Arc::clone(series));
                let series = vec![Series::of(series.pyramid())];
                (inner, series)
            }
            Source::File { path, series } => Inner::open_file(path, *series)?,
        };
        let declared = match &inner {
            Inner::Memory(series) => series.pyramid(),
            Inner::Zarr(image) => image.pyramid(),
            Inner::Tiff(image) => image.pyramid(),
            Inner::Czi(image) => image.pyramid(),
        };
        let pyramid = level::extended(
            declared.levels(),
            declared.channels(),
            declared.frame_count(),
            declared.frame_interval(),
        );
        let identity = cache::identity(source, &geometry(&pyramid));
        let store = match source {
            Source::Memory(_) => None,
            Source::File { .. } => cache.as_deref().and_then(Store::open),
        };
        Ok(Self {
            inner,
            pyramid,
            series,
            selected: source.series(),
            identity,
            tiles: Mutex::new(Memory::new()),
            store,
        })
    }

    pub fn series(&self) -> &[Series] {
        &self.series
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn name(&self) -> &str {
        match &self.inner {
            Inner::Memory(_) => "synthetic",
            Inner::Zarr(image) => image.name(),
            Inner::Tiff(image) => image.name(),
            Inner::Czi(image) => image.name(),
        }
    }

    pub fn pyramid(&self) -> &Pyramid {
        &self.pyramid
    }

    pub fn format(&self) -> Option<Format> {
        match &self.inner {
            Inner::Memory(_) => None,
            Inner::Zarr(_) => Some(Format::Zarr),
            Inner::Tiff(_) => Some(Format::Tiff),
            Inner::Czi(_) => Some(Format::Czi),
        }
    }

    pub fn read_region(
        &self,
        time: u32,
        level: usize,
        channel: usize,
        ranges: &[Range<u32>; 3],
    ) -> Result<VoxelData, Error> {
        self.level_extent(level)?;
        if level < self.pyramid.stored() {
            return self.stored_region(time, level, channel, ranges);
        }
        self.resolved_region(time, level, channel, ranges)
    }

    pub fn read_brick(
        &self,
        time: u32,
        level: usize,
        coord: BrickCoord,
    ) -> Result<BrickData, Error> {
        if level < self.pyramid.stored() {
            return self.stored_brick(time, level, coord);
        }
        let extent = self.level_extent(level)?;
        let grid = BrickGrid::new(extent);
        let counts = grid.counts();
        if coord.x() >= counts[0] || coord.y() >= counts[1] || coord.z() >= counts[2] {
            return Err(Error::Resolved {
                detail: "the brick lies outside the resolved level".to_string(),
            });
        }
        let spatial = grid.padded_range(coord);
        let region = Extent::new(
            spatial[0].end - spatial[0].start,
            spatial[1].end - spatial[1].start,
            spatial[2].end - spatial[2].start,
        );
        let origin = [spatial[0].start, spatial[1].start, spatial[2].start];
        let mut bricks = Vec::with_capacity(self.pyramid.channel_count());
        for channel in 0..self.pyramid.channel_count() {
            let data = self.resolved_region(time, level, channel, &spatial)?;
            bricks.push(pad_brick(&data, origin, region, grid.origin(coord), extent));
        }
        Ok(BrickData::new(bricks))
    }

    fn stored_region(
        &self,
        time: u32,
        level: usize,
        channel: usize,
        ranges: &[Range<u32>; 3],
    ) -> Result<VoxelData, Error> {
        match &self.inner {
            Inner::Memory(series) => Ok(series.region(
                time,
                channel,
                ranges[0].clone(),
                ranges[1].clone(),
                ranges[2].clone(),
            )),
            Inner::Zarr(image) => Ok(image.read_region(
                time,
                level,
                channel,
                ranges[0].clone(),
                ranges[1].clone(),
                ranges[2].clone(),
            )?),
            Inner::Tiff(image) => Ok(image.read_region(
                time,
                level,
                channel,
                ranges[0].clone(),
                ranges[1].clone(),
                ranges[2].clone(),
            )?),
            Inner::Czi(image) => Ok(image.read_region(
                time,
                level,
                channel,
                ranges[0].clone(),
                ranges[1].clone(),
                ranges[2].clone(),
            )?),
        }
    }

    fn stored_brick(&self, time: u32, level: usize, coord: BrickCoord) -> Result<BrickData, Error> {
        match &self.inner {
            Inner::Memory(series) => {
                let grid = BrickGrid::new(series.pyramid().level(level).extent());
                Ok(series.brick(time, &grid, coord))
            }
            Inner::Zarr(image) => Ok(image.read_brick(time, level, coord)?),
            Inner::Tiff(image) => Ok(image.read_brick(time, level, coord)?),
            Inner::Czi(image) => Ok(image.read_brick(time, level, coord)?),
        }
    }

    fn resolved_region(
        &self,
        time: u32,
        level: usize,
        channel: usize,
        ranges: &[Range<u32>; 3],
    ) -> Result<VoxelData, Error> {
        let scale = Scale::of(&self.pyramid, level);
        if time >= self.pyramid.frame_count() {
            return Err(Error::Resolved {
                detail: "the requested frame does not exist".to_string(),
            });
        }
        let Some(info) = self.pyramid.channels().get(channel) else {
            return Err(Error::Resolved {
                detail: "the requested channel does not exist".to_string(),
            });
        };
        if !scale.within(ranges) {
            return Err(Error::Resolved {
                detail: "the region lies outside the resolved level".to_string(),
            });
        }
        let grid = Grid::of(scale.extent());
        let mut data = tile::allocate(info.format(), ranges);
        let (first, last) = grid.cover(ranges);
        for z in first[2]..=last[2] {
            for y in first[1]..=last[1] {
                for x in first[0]..=last[0] {
                    let coord = [x, y, z];
                    let covered = grid.range(coord);
                    if let Some(tile) =
                        self.cached(time, level, channel, coord, &grid, info.format())
                    {
                        tile::copy_intersection(&mut data, ranges, &tile, &covered);
                        continue;
                    }
                    let overlap = intersection(ranges, &covered);
                    let partial = volume(&overlap) * PARTIAL_DIVISOR <= volume(&covered);
                    if partial {
                        let composed =
                            self.compose(time, level, channel, &overlap, info.format())?;
                        tile::copy_intersection(&mut data, ranges, &composed, &overlap);
                    } else {
                        let tile = self.tile(time, level, channel, coord, &grid, info.format())?;
                        tile::copy_intersection(&mut data, ranges, &tile, &covered);
                    }
                }
            }
        }
        Ok(data)
    }

    fn cached(
        &self,
        time: u32,
        level: usize,
        channel: usize,
        coord: [u32; 3],
        grid: &Grid,
        format: VoxelFormat,
    ) -> Option<Arc<VoxelData>> {
        let key = self.key(time, level, channel, coord);
        if let Some(data) = self.tiles().fetch(key) {
            return Some(data);
        }
        let shape = Shape {
            extent: grid.extent_of(coord),
            format,
        };
        let data = self
            .store
            .as_ref()
            .and_then(|store| store.load(key, shape))?;
        let data = Arc::new(data);
        self.tiles().insert(key, Arc::clone(&data));
        Some(data)
    }

    fn key(&self, time: u32, level: usize, channel: usize, coord: [u32; 3]) -> Key {
        Key {
            identity: self.identity,
            level: u32::try_from(level).expect("a pyramid holds fewer than 4G levels"),
            time,
            channel: u32::try_from(channel).expect("a pyramid holds fewer than 4G channels"),
            coord,
        }
    }

    fn tile(
        &self,
        time: u32,
        level: usize,
        channel: usize,
        coord: [u32; 3],
        grid: &Grid,
        format: VoxelFormat,
    ) -> Result<Arc<VoxelData>, Error> {
        if let Some(data) = self.cached(time, level, channel, coord, grid, format) {
            return Ok(data);
        }
        let key = self.key(time, level, channel, coord);
        let shape = Shape {
            extent: grid.extent_of(coord),
            format,
        };
        let data = Arc::new(self.synthesize(time, level, channel, coord, grid, format)?);
        if let Some(store) = self.store.as_ref() {
            store.save(key, shape, &data);
        }
        self.tiles().insert(key, Arc::clone(&data));
        Ok(data)
    }

    fn synthesize(
        &self,
        time: u32,
        level: usize,
        channel: usize,
        coord: [u32; 3],
        grid: &Grid,
        format: VoxelFormat,
    ) -> Result<VoxelData, Error> {
        self.compose(time, level, channel, &grid.range(coord), format)
    }

    fn compose(
        &self,
        time: u32,
        level: usize,
        channel: usize,
        ranges: &[Range<u32>; 3],
        format: VoxelFormat,
    ) -> Result<VoxelData, Error> {
        let scale = Scale::of(&self.pyramid, level);
        let source = scale.source_ranges(ranges);
        let base = scale.base(ranges);
        let extent = Extent::new(
            ranges[0].end - ranges[0].start,
            ranges[1].end - ranges[1].start,
            ranges[2].end - ranges[2].start,
        );
        let mut field = Field::new(extent);
        level::chunks(&source, |chunk| {
            let data = self.stored_region(time, scale.source(), channel, &chunk)?;
            field.add(
                &data,
                Extent::new(
                    chunk[0].end - chunk[0].start,
                    chunk[1].end - chunk[1].start,
                    chunk[2].end - chunk[2].start,
                ),
                [
                    chunk[0].start - base[0],
                    chunk[1].start - base[1],
                    chunk[2].start - base[2],
                ],
                scale.shift(),
            );
            Ok::<(), Error>(())
        })?;
        Ok(field.finish(format))
    }

    fn level_extent(&self, level: usize) -> Result<Extent, Error> {
        if level >= self.pyramid.level_count() {
            return Err(Error::Resolved {
                detail: "the requested resolution level does not exist".to_string(),
            });
        }
        Ok(self.pyramid.level(level).extent())
    }

    fn tiles(&self) -> MutexGuard<'_, Memory> {
        self.tiles.lock().unwrap_or_else(|error| error.into_inner())
    }
}

fn intersection(left: &[Range<u32>; 3], right: &[Range<u32>; 3]) -> [Range<u32>; 3] {
    [0, 1, 2]
        .map(|axis| left[axis].start.max(right[axis].start)..left[axis].end.min(right[axis].end))
}

fn volume(ranges: &[Range<u32>; 3]) -> u64 {
    ranges
        .iter()
        .map(|range| u64::from(range.end.saturating_sub(range.start)))
        .product()
}

fn geometry(pyramid: &Pyramid) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&RESOLVED_VERSION.to_le_bytes());
    bytes.extend_from_slice(&(pyramid.stored() as u64).to_le_bytes());
    bytes.extend_from_slice(&(pyramid.frame_count() as u64).to_le_bytes());
    for level in pyramid.levels() {
        let extent = level.extent();
        for value in [extent.x(), extent.y(), extent.z()] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        let spacing = level.spacing();
        for value in [spacing.x(), spacing.y(), spacing.z()] {
            bytes.extend_from_slice(&value.to_bits().to_le_bytes());
        }
    }
    for channel in pyramid.channels() {
        bytes.extend_from_slice(channel.name().as_bytes());
        bytes.push(match channel.format() {
            VoxelFormat::U8 => 0,
            VoxelFormat::U16 => 1,
            VoxelFormat::F32 => 2,
        });
    }
    bytes
}

fn is_tiff(path: &Path) -> Result<bool, Error> {
    magic(path, 4).map(|magic| {
        magic
            .as_deref()
            .is_some_and(|magic| matches!(magic, b"II*\0" | b"MM\0*" | b"II+\0" | b"MM\0+"))
    })
}

fn is_czi(path: &Path) -> Result<bool, Error> {
    magic(path, 10).map(|magic| magic.as_deref().is_some_and(|magic| magic == b"ZISRAWFILE"))
}

fn magic(path: &Path, length: usize) -> Result<Option<Vec<u8>>, Error> {
    let mut file = File::open(path).map_err(|error| Error::Open {
        path: path.to_path_buf(),
        detail: error.to_string(),
    })?;
    let mut bytes = vec![0u8; length];
    if file.read_exact(&mut bytes).is_err() {
        return Ok(None);
    }
    Ok(Some(bytes))
}

fn is_zarr(path: &Path) -> bool {
    [".zarray", ".zgroup", "zarr.json"]
        .iter()
        .any(|name| path.join(name).exists())
}

pub fn is_image(path: &Path) -> bool {
    if path.is_dir() {
        is_zarr(path)
    } else {
        is_czi(path).unwrap_or(false) || is_tiff(path).unwrap_or(false)
    }
}

pub fn series_count(path: &Path) -> Result<usize, Error> {
    if path.is_dir() || !is_czi(path)? {
        return Ok(1);
    }
    Ok(grana_czi::scene_count(path)?)
}

pub fn dataset_name(path: &Path) -> String {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return "dataset".to_string();
    };
    if let Some(stem) = name.strip_suffix(".zarr") {
        return stem.to_string();
    }
    if name.to_ascii_lowercase().ends_with(".czi") {
        return grana_czi::dataset_name(Path::new(name));
    }
    grana_tiff::dataset_name(Path::new(name))
}

impl Inner {
    fn open_file(path: &Path, series: usize) -> Result<(Self, Vec<Series>), Error> {
        if path.is_dir() {
            let image = grana_zarr::Image::open(path)?;
            let series = single_series(image.pyramid(), path, series)?;
            return Ok((Self::Zarr(image), series));
        }
        if is_czi(path)? {
            let image = grana_czi::Image::open(path, series)?;
            let scenes = image
                .scenes()
                .iter()
                .map(|scene| {
                    Series::new(
                        scene.extent(),
                        scene.channels(),
                        scene.frames(),
                        scene.name().map(str::to_string),
                    )
                })
                .collect();
            return Ok((Self::Czi(image), scenes));
        }
        if !is_tiff(path)? {
            return Err(Error::Unsupported {
                path: path.to_path_buf(),
                detail: "the file is neither an OME-TIFF nor a CZI image".to_string(),
            });
        }
        let image = grana_tiff::Image::open(path)?;
        let series = single_series(image.pyramid(), path, series)?;
        Ok((Self::Tiff(image), series))
    }
}

fn single_series(pyramid: &Pyramid, path: &Path, series: usize) -> Result<Vec<Series>, Error> {
    if series > 0 {
        return Err(Error::Unsupported {
            path: path.to_path_buf(),
            detail: format!("the image holds a single series and series {series} was requested"),
        });
    }
    Ok(vec![Series::of(pyramid)])
}
