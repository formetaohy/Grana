use std::fs::File;
use std::io::Read;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use grana_volume::{
    BrickCoord, BrickData, BrickGrid, Extent, Pyramid, VolumeSeries, VoxelData, VoxelFormat,
    pad_brick,
};
use grana_zarr::Labels;

use crate::align;
use crate::cache::{self, Cache, Key, Shape, Store};
use crate::level::{self, Field, Scale};
use crate::region;
use crate::{Error, Series, Source};

const RESOLVED_VERSION: u32 = 2;
const PARTIAL_DIVISOR: u64 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Zarr,
    Tiff,
    Czi,
    Nikon,
    Leica,
}

enum Inner {
    Memory(Arc<VolumeSeries>),
    Zarr(grana_zarr::Image),
    Tiff(grana_tiff::Image),
    Czi(grana_czi::Image),
    Readout(grana_readout::Image),
}

pub struct Image {
    inner: Inner,
    pyramid: Pyramid,
    series: Vec<Series>,
    selected: usize,
    identity: u64,
    bricks: Cache,
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
            Source::File {
                path,
                series,
                group,
            } => Inner::open_file(path, *series, group.as_deref())?,
        };
        let declared = match &inner {
            Inner::Memory(series) => series.pyramid(),
            Inner::Zarr(image) => image.pyramid(),
            Inner::Tiff(image) => image.pyramid(),
            Inner::Czi(image) => image.pyramid(),
            Inner::Readout(image) => image.pyramid(),
        };
        let pyramid = level::extended(declared);
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
            bricks: Cache::new(),
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
            Inner::Readout(image) => image.name(),
        }
    }

    pub fn pyramid(&self) -> &Pyramid {
        &self.pyramid
    }

    pub fn labels(&self) -> Result<Labels, Error> {
        match &self.inner {
            Inner::Zarr(image) => Ok(image.labels()?),
            Inner::Memory(_) | Inner::Tiff(_) | Inner::Czi(_) | Inner::Readout(_) => {
                Ok(Labels::empty())
            }
        }
    }

    pub fn format(&self) -> Option<Format> {
        match &self.inner {
            Inner::Memory(_) => None,
            Inner::Zarr(_) => Some(Format::Zarr),
            Inner::Tiff(_) => Some(Format::Tiff),
            Inner::Czi(_) => Some(Format::Czi),
            Inner::Readout(image) => Some(match image.family() {
                grana_readout::Family::Nikon => Format::Nikon,
                grana_readout::Family::Leica => Format::Leica,
            }),
        }
    }

    pub fn read_region(
        &self,
        time: u32,
        level: usize,
        channel: usize,
        ranges: &[Range<u32>; 3],
        shift: [f32; 3],
    ) -> Result<VoxelData, Error> {
        self.level_extent(level)?;
        let shift = align::level_shift(shift, level);
        if shift == [0.0; 3] {
            return self.region_at(time, level, channel, ranges);
        }
        self.aligned_region(time, level, channel, ranges, shift)
    }

    pub fn read_brick(
        &self,
        time: u32,
        level: usize,
        coord: BrickCoord,
        shift: [f32; 3],
    ) -> Result<BrickData, Error> {
        if shift == [0.0; 3] && level < self.pyramid.stored() {
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
        let covered = covered_extent(&spatial);
        let origin = [spatial[0].start, spatial[1].start, spatial[2].start];
        let mut bricks = Vec::with_capacity(self.pyramid.channel_count());
        for channel in 0..self.pyramid.channel_count() {
            let data = self.brick_region(time, level, channel, coord, shift)?;
            bricks.push(pad_brick(
                &data,
                origin,
                covered,
                grid.origin(coord),
                extent,
            ));
        }
        Ok(BrickData::new(bricks))
    }

    fn brick_region(
        &self,
        time: u32,
        level: usize,
        channel: usize,
        coord: BrickCoord,
        shift: [f32; 3],
    ) -> Result<Arc<VoxelData>, Error> {
        let extent = self.level_extent(level)?;
        let grid = BrickGrid::new(extent);
        let spatial = grid.padded_range(coord);
        let shift = align::level_shift(shift, level);
        if shift != [0.0; 3] {
            return Ok(Arc::new(
                self.aligned_region(time, level, channel, &spatial, shift)?,
            ));
        }
        if level < self.pyramid.stored() {
            return Ok(Arc::new(
                self.stored_region(time, level, channel, &spatial)?,
            ));
        }
        let format = self.channel_format(channel)?;
        self.resolved_brick(time, level, channel, coord, format)
    }

    fn resolved_brick(
        &self,
        time: u32,
        level: usize,
        channel: usize,
        coord: BrickCoord,
        format: VoxelFormat,
    ) -> Result<Arc<VoxelData>, Error> {
        let extent = self.level_extent(level)?;
        let grid = BrickGrid::new(extent);
        let spatial = grid.padded_range(coord);
        let covered = covered_extent(&spatial);
        let key = self.key(time, level, channel, [coord.x(), coord.y(), coord.z()]);
        if let Some(data) = self.held(key, covered, format) {
            return Ok(data);
        }
        let data = Arc::new(self.compose(time, level, channel, &spatial, format)?);
        if let Some(store) = self.store.as_ref() {
            store.save(
                key,
                Shape {
                    extent: covered,
                    format,
                },
                &data,
            );
        }
        self.bricks.insert(key, Arc::clone(&data));
        Ok(data)
    }

    fn region_at(
        &self,
        time: u32,
        level: usize,
        channel: usize,
        ranges: &[Range<u32>; 3],
    ) -> Result<VoxelData, Error> {
        if level < self.pyramid.stored() {
            return self.stored_region(time, level, channel, ranges);
        }
        self.resolved_region(time, level, channel, ranges)
    }

    fn aligned_region(
        &self,
        time: u32,
        level: usize,
        channel: usize,
        ranges: &[Range<u32>; 3],
        shift: [f32; 3],
    ) -> Result<VoxelData, Error> {
        let format = self.channel_format(channel)?;
        let extent = self.level_extent(level)?;
        let limits = [extent.x(), extent.y(), extent.z()];
        let plan = align::plan(ranges, shift, limits);
        if plan.source.iter().any(|range| range.start >= range.end) {
            return Ok(align::zeros(format, ranges));
        }
        let data = self.region_at(time, level, channel, &plan.source)?;
        Ok(align::resample(&data, &plan, ranges))
    }

    fn channel_format(&self, channel: usize) -> Result<VoxelFormat, Error> {
        self.pyramid
            .channels()
            .get(channel)
            .map(grana_volume::ChannelInfo::format)
            .ok_or_else(|| Error::Resolved {
                detail: "the requested channel does not exist".to_string(),
            })
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
            Inner::Readout(image) => Ok(image.read_region(
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
            Inner::Readout(image) => Ok(image.read_brick(time, level, coord)?),
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
        let grid = BrickGrid::new(scale.extent());
        let mut output = region::allocate(info.format(), ranges);
        let (first, last) = grid.cover(ranges);
        for z in first[2]..=last[2] {
            for y in first[1]..=last[1] {
                for x in first[0]..=last[0] {
                    let coord = [x, y, z];
                    let covered = grid.padded_range(BrickCoord::new(x, y, z));
                    let overlap = intersection(ranges, &covered);
                    if overlap[0].start >= overlap[0].end
                        || overlap[1].start >= overlap[1].end
                        || overlap[2].start >= overlap[2].end
                    {
                        continue;
                    }
                    let key = self.key(time, level, channel, coord);
                    if let Some(brick) = self.held(key, covered_extent(&covered), info.format()) {
                        region::copy_intersection(&mut output, ranges, &brick, &covered);
                        continue;
                    }
                    let partial = volume(&overlap) * PARTIAL_DIVISOR <= volume(&covered);
                    if partial {
                        let composed =
                            self.compose(time, level, channel, &overlap, info.format())?;
                        region::copy_intersection(&mut output, ranges, &composed, &overlap);
                    } else {
                        let brick = self.compose(time, level, channel, &covered, info.format())?;
                        region::copy_intersection(&mut output, ranges, &brick, &covered);
                        self.hold(key, covered_extent(&covered), info.format(), brick);
                    }
                }
            }
        }
        Ok(output)
    }

    fn held(&self, key: Key, extent: Extent, format: VoxelFormat) -> Option<Arc<VoxelData>> {
        if let Some(data) = self.bricks.fetch(key) {
            return Some(data);
        }
        let data = self
            .store
            .as_ref()
            .and_then(|store| store.load(key, Shape { extent, format }))?;
        let data = Arc::new(data);
        self.bricks.insert(key, Arc::clone(&data));
        Some(data)
    }

    fn hold(&self, key: Key, extent: Extent, format: VoxelFormat, data: VoxelData) {
        let data = Arc::new(data);
        if let Some(store) = self.store.as_ref() {
            store.save(key, Shape { extent, format }, &data);
        }
        self.bricks.insert(key, data);
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
}

fn covered_extent(ranges: &[Range<u32>; 3]) -> Extent {
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
        for value in level.origin() {
            bytes.extend_from_slice(&value.to_bits().to_le_bytes());
        }
    }
    for value in pyramid.placement() {
        bytes.extend_from_slice(&value.to_bits().to_le_bytes());
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
        is_czi(path).unwrap_or(false)
            || is_tiff(path).unwrap_or(false)
            || grana_readout::is_supported(path)
    }
}

pub fn series_count(path: &Path) -> Result<usize, Error> {
    if path.is_dir() {
        return plate_of(path).map(|plate| plate.map_or(1, |plate| plate.len().max(1)));
    }
    if is_czi(path)? {
        return Ok(grana_czi::scene_count(path)?);
    }
    if grana_readout::is_supported(path) {
        return Ok(grana_readout::scene_count(path)?);
    }
    Ok(1)
}

pub fn series_names(path: &Path) -> Result<Vec<Option<String>>, Error> {
    if path.is_dir() {
        return Ok(plate_of(path)?
            .map(|plate| {
                plate
                    .fields()
                    .iter()
                    .map(|field| Some(field.name().to_string()))
                    .collect()
            })
            .unwrap_or_default());
    }
    if is_czi(path)? {
        return Ok(grana_czi::scenes(path)?
            .into_iter()
            .map(|scene| scene.name().map(str::to_string))
            .collect());
    }
    if grana_readout::is_supported(path) {
        return Ok(grana_readout::scenes(path)?
            .into_iter()
            .map(|scene| scene.name().map(str::to_string))
            .collect());
    }
    Ok(Vec::new())
}

pub fn series_group(path: &Path, series: usize) -> Result<Option<String>, Error> {
    if !path.is_dir() {
        return Ok(None);
    }
    let Some(plate) = plate_of(path)? else {
        return Ok(None);
    };
    let field = plate.field(series).ok_or_else(|| Error::Unsupported {
        path: path.to_path_buf(),
        detail: format!(
            "the plate holds {} fields of view and field {series} was requested",
            plate.len()
        ),
    })?;
    Ok(Some(field.path().to_string()))
}

fn plate_of(path: &Path) -> Result<Option<grana_zarr::Plate>, Error> {
    if grana_zarr::group_kind(path)? != grana_zarr::GroupKind::Plate {
        return Ok(None);
    }
    let plate = grana_zarr::open_plate(path)?;
    if plate.is_empty() {
        return Err(Error::Unsupported {
            path: path.to_path_buf(),
            detail: "the OME-Zarr plate holds no field of view".to_string(),
        });
    }
    Ok(Some(plate))
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
    if grana_readout::has_extension(Path::new(name)) {
        return grana_readout::dataset_name(Path::new(name));
    }
    grana_tiff::dataset_name(Path::new(name))
}

impl Inner {
    fn open_file(
        path: &Path,
        series: usize,
        group: Option<&str>,
    ) -> Result<(Self, Vec<Series>), Error> {
        if path.is_dir() {
            if let Some(group) = group {
                let image = grana_zarr::Image::open_group(path, group)?;
                let series = vec![Series::of(image.pyramid())];
                return Ok((Self::Zarr(image), series));
            }
            if grana_zarr::group_kind(path)? == grana_zarr::GroupKind::Plate {
                return Err(Error::Unsupported {
                    path: path.to_path_buf(),
                    detail: "the OME-Zarr plate lists its fields of view as series".to_string(),
                });
            }
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
        if grana_readout::is_supported(path) {
            let image = grana_readout::Image::open(path, series)?;
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
            return Ok((Self::Readout(image), scenes));
        }
        if !is_tiff(path)? {
            return Err(Error::Unsupported {
                path: path.to_path_buf(),
                detail: "the file is not an OME-TIFF, a CZI, an ND2 or a LIF image".to_string(),
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
