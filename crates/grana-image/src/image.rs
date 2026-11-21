use std::fs::File;
use std::io::Read;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use grana_volume::{
    BrickCoord, BrickData, BrickGrid, Extent, Placement, Pyramid, VolumeSeries, VoxelData,
    VoxelFormat, VoxelSemantics, pad_brick,
};
use grana_warp::{Warp, Warps};
use grana_zarr::{Labels, Plate};
use rayon::prelude::*;

use crate::align;
use crate::cache::{self, Cache, Key, Shape, Store};
use crate::level::Scale;
use crate::plane;
use crate::region;
use crate::{Error, Series, Source};

const RESOLVED_VERSION: u32 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Zarr,
    Tiff,
    Czi,
    Readout,
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
    identity: Option<u64>,
    bricks: Cache,
    planes: plane::Caches,
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
            Source::Remote { url, series, group } => {
                Inner::open_remote(url, *series, group.as_deref())?
            }
        };
        let declared = match &inner {
            Inner::Memory(series) => series.pyramid(),
            Inner::Zarr(image) => image.pyramid(),
            Inner::Tiff(image) => image.pyramid(),
            Inner::Czi(image) => image.pyramid(),
            Inner::Readout(image) => image.pyramid(),
        };
        let pyramid = crate::level::extended(declared);
        let dataset = match &inner {
            Inner::Zarr(image) => image.identity(),
            _ => None,
        };
        let identity = cache::identity(source, &geometry(&pyramid), dataset);
        let store = match (source, identity) {
            (Source::Memory(_), _) | (_, None) => None,
            (Source::File { .. } | Source::Remote { .. }, Some(_)) => {
                cache.as_deref().and_then(Store::open)
            }
        };
        Ok(Self {
            inner,
            planes: plane::Caches::of(&pyramid),
            pyramid,
            series,
            selected: source.series(),
            identity,
            bricks: cache::bricks(),
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

    pub fn transfer(&self) -> Option<Arc<grana_zarr::Transfer>> {
        match &self.inner {
            Inner::Zarr(image) => image.transfer(),
            Inner::Memory(_) | Inner::Tiff(_) | Inner::Czi(_) | Inner::Readout(_) => None,
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
            Inner::Readout(_) => Some(Format::Readout),
        }
    }

    pub fn family(&self) -> Option<grana_readout::Family> {
        match &self.inner {
            Inner::Readout(image) => Some(image.family()),
            Inner::Memory(_) | Inner::Zarr(_) | Inner::Tiff(_) | Inner::Czi(_) => None,
        }
    }

    pub fn read_region(
        &self,
        time: u32,
        level: usize,
        channel: usize,
        ranges: &[Range<u32>; 3],
        warp: &Warp,
    ) -> Result<VoxelData, Error> {
        self.level_extent(level)?;
        if warp.identity() {
            return self.region_at(time, level, channel, ranges);
        }
        self.aligned_region(time, level, channel, ranges, warp)
    }

    pub fn read_brick(
        &self,
        time: u32,
        level: usize,
        coord: BrickCoord,
        warps: &Warps,
    ) -> Result<BrickData, Error> {
        let extent = self.level_extent(level)?;
        let grid = BrickGrid::new(extent);
        let counts = grid.counts();
        if coord.x() >= counts[0] || coord.y() >= counts[1] || coord.z() >= counts[2] {
            return Err(Error::Resolved {
                detail: "the brick lies outside the level".to_string(),
            });
        }
        if level < self.pyramid.stored() {
            let bricks = (0..self.pyramid.channel_count())
                .into_par_iter()
                .map(|channel| {
                    self.brick_region(time, level, channel, coord, warps.get(channel))
                        .map(|data| (*data).clone())
                })
                .collect::<Result<Vec<_>, Error>>()?;
            return Ok(BrickData::new(bricks));
        }
        self.resolved_brick(time, level, coord, warps)
    }

    fn brick_region(
        &self,
        time: u32,
        level: usize,
        channel: usize,
        coord: BrickCoord,
        warp: &Warp,
    ) -> Result<Arc<VoxelData>, Error> {
        let extent = self.level_extent(level)?;
        let grid = BrickGrid::new(extent);
        let spatial = grid.padded_range(coord);
        let covered = covered_extent(&spatial);
        let origin = [spatial[0].start, spatial[1].start, spatial[2].start];
        let pad = |data: &VoxelData| pad_brick(data, origin, covered, grid.origin(coord), extent);
        if !warp.identity() {
            let data = self.aligned_region(time, level, channel, &spatial, warp)?;
            return Ok(Arc::new(pad(&data)));
        }
        let key = self.key(time, level, channel, [coord.x(), coord.y(), coord.z()]);
        if let Some(data) = self.bricks.fetch(key) {
            return Ok(data);
        }
        let region = Arc::new(self.stored_region(time, level, channel, &spatial)?);
        let data = Arc::new(pad(&region));
        self.bricks.insert(key, Arc::clone(&data));
        Ok(data)
    }

    fn resolved_brick(
        &self,
        time: u32,
        level: usize,
        coord: BrickCoord,
        warps: &Warps,
    ) -> Result<BrickData, Error> {
        let channels = (0..self.pyramid.channel_count())
            .map(|channel| {
                self.resolved_channel(time, level, channel, coord, warps.get(channel))
                    .map(|data| (*data).clone())
            })
            .collect::<Result<Vec<_>, Error>>()?;
        Ok(BrickData::new(channels))
    }

    fn resolved_channel(
        &self,
        time: u32,
        level: usize,
        channel: usize,
        coord: BrickCoord,
        warp: &Warp,
    ) -> Result<Arc<VoxelData>, Error> {
        let extent = self.level_extent(level)?;
        let grid = BrickGrid::new(extent);
        let spatial = grid.padded_range(coord);
        let covered = covered_extent(&spatial);
        let origin = [spatial[0].start, spatial[1].start, spatial[2].start];
        if !warp.identity() {
            let data = self.aligned_region(time, level, channel, &spatial, warp)?;
            return Ok(Arc::new(pad_brick(
                &data,
                origin,
                covered,
                grid.origin(coord),
                extent,
            )));
        }
        let key = self.key(time, level, channel, [coord.x(), coord.y(), coord.z()]);
        if let Some(data) = self.bricks.fetch(key) {
            return Ok(data);
        }
        let (region, _) = self.covered_channel(time, level, channel, coord)?;
        let data = Arc::new(pad_brick(
            &region,
            origin,
            covered,
            grid.origin(coord),
            extent,
        ));
        self.bricks.insert(key, Arc::clone(&data));
        Ok(data)
    }

    fn covered_channel(
        &self,
        time: u32,
        level: usize,
        channel: usize,
        coord: BrickCoord,
    ) -> Result<(Arc<VoxelData>, [Range<u32>; 3]), Error> {
        let extent = self.level_extent(level)?;
        let grid = BrickGrid::new(extent);
        let covered = grid.padded_range(coord);
        let format = self.channel_format(channel)?;
        let key = self.key(time, level, channel, [coord.x(), coord.y(), coord.z()]);
        let shape = covered_extent(&covered);
        let stamp = self.source_stamp(time, level, channel, &covered)?;
        if let Some(data) = self.held(key, shape, format, stamp) {
            return Ok((data, covered));
        }
        let data = Arc::new(self.compose(time, level, channel, &covered)?);
        self.hold(key, shape, format, stamp, Arc::clone(&data));
        Ok((data, covered))
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
        warp: &Warp,
    ) -> Result<VoxelData, Error> {
        let format = self.channel_format(channel)?;
        let extent = self.level_extent(level)?;
        let info = self.pyramid.level(level);
        let semantics = self.pyramid.channels()[channel].semantics();
        let limits = [extent.x(), extent.y(), extent.z()];
        let plan = align::plan(ranges, info, warp, limits);
        if plan.source().iter().any(|range| range.start >= range.end) {
            return Ok(align::zeros(format, ranges));
        }
        let data = self.region_at(time, level, channel, plan.source())?;
        Ok(align::resample(&data, &plan, ranges, semantics))
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

    fn resolved_region(
        &self,
        time: u32,
        level: usize,
        channel: usize,
        ranges: &[Range<u32>; 3],
    ) -> Result<VoxelData, Error> {
        let extent = self.level_extent(level)?;
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
        if ranges[0].start >= ranges[0].end
            || ranges[0].end > extent.x()
            || ranges[1].start >= ranges[1].end
            || ranges[1].end > extent.y()
            || ranges[2].start >= ranges[2].end
            || ranges[2].end > extent.z()
        {
            return Err(Error::Resolved {
                detail: "the region lies outside the resolved level".to_string(),
            });
        }
        let mut output = region::allocate(info.format(), ranges);
        let grid = BrickGrid::new(extent);
        let (first, last) = grid.cover(ranges);
        for z in first[2]..=last[2] {
            for y in first[1]..=last[1] {
                for x in first[0]..=last[0] {
                    let coord = BrickCoord::new(x, y, z);
                    let (data, covered) = self.covered_channel(time, level, channel, coord)?;
                    region::copy_intersection(&mut output, ranges, &data, &covered);
                }
            }
        }
        Ok(output)
    }

    fn plane(&self, time: u32, level: usize, z: u32) -> Result<Arc<plane::Plane>, Error> {
        let key = plane::Key {
            level: u32::try_from(level).expect("a pyramid holds fewer than 4G levels"),
            time,
            z,
        };
        let cache = &self.planes;
        if let Some(held) = cache.fetch(key) {
            return Ok(held);
        }
        let built = Arc::new(self.build_plane(time, level, z)?);
        cache.insert(key, Arc::clone(&built));
        Ok(built)
    }

    fn build_plane(&self, time: u32, level: usize, z: u32) -> Result<plane::Plane, Error> {
        let extent = self.level_extent(level)?;
        if time >= self.pyramid.frame_count() {
            return Err(Error::Resolved {
                detail: "the requested frame does not exist".to_string(),
            });
        }
        if z >= extent.z() {
            return Err(Error::Resolved {
                detail: "the requested plane lies outside the level".to_string(),
            });
        }
        let channels = self.pyramid.channel_count();
        if level < self.pyramid.stored() {
            let mut planes = Vec::with_capacity(channels);
            for channel in 0..channels {
                planes.push(self.stored_region(
                    time,
                    level,
                    channel,
                    &[0..extent.x(), 0..extent.y(), z..z + 1],
                )?);
            }
            return Ok(plane::Plane::new(planes));
        }
        let parent = level - 1;
        let source = self.level_extent(parent)?;
        let width = extent.x() as usize;
        let height = extent.y() as usize;
        let start = z * 2;
        let end = ((z + 1) * 2).min(source.z());
        let mut held = Vec::with_capacity((end - start) as usize);
        for source_z in start..end {
            held.push(self.plane(time, parent, source_z)?);
        }
        let mut planes = Vec::with_capacity(channels);
        for channel in 0..channels {
            let format = self.channel_format(channel)?;
            let semantics = self
                .pyramid
                .channels()
                .get(channel)
                .map_or(VoxelSemantics::Scalar, |info| info.semantics());
            let sources = held
                .iter()
                .map(|plane| plane.channel(channel))
                .collect::<Vec<_>>();
            let mut output = VoxelData::blank(format, width * height);
            let mut row = 0;
            while row < height {
                let next = (row + plane::STRIP_ROWS).min(height);
                let strip = plane::reduce(format, semantics, source, width, row..next, &sources);
                output.blit(
                    &strip,
                    Placement::new(width as u32, [0, 0]),
                    Placement::new(width as u32, [0, row as u32]),
                    width as u32,
                    (next - row) as u32,
                );
                row = next;
            }
            planes.push(output);
        }
        Ok(plane::Plane::new(planes))
    }

    fn compose(
        &self,
        time: u32,
        level: usize,
        channel: usize,
        ranges: &[Range<u32>; 3],
    ) -> Result<VoxelData, Error> {
        let extent = self.level_extent(level)?;
        let width = extent.x() as usize;
        let rows = (ranges[1].end - ranges[1].start) as usize;
        let span = (ranges[0].end - ranges[0].start) as usize;
        let mut output = region::allocate(self.channel_format(channel)?, ranges);
        for z in ranges[2].clone() {
            let held = self.plane(time, level, z)?;
            let row = ((z - ranges[2].start) as usize) * rows;
            region::copy_rows(
                &mut output,
                span,
                row,
                held.channel(channel),
                width,
                ranges[0].clone(),
                ranges[1].clone(),
            );
        }
        Ok(output)
    }

    fn held(
        &self,
        key: Key,
        extent: Extent,
        format: VoxelFormat,
        stamp: Option<u64>,
    ) -> Option<Arc<VoxelData>> {
        let store = self.store.as_ref()?;
        let data = store.load(key, Shape { extent, format }, stamp?)?;
        Some(Arc::new(data))
    }

    fn hold(
        &self,
        key: Key,
        extent: Extent,
        format: VoxelFormat,
        stamp: Option<u64>,
        data: Arc<VoxelData>,
    ) {
        if let (Some(store), Some(stamp)) = (self.store.as_ref(), stamp) {
            store.save(key, Shape { extent, format }, stamp, &data);
        }
    }

    fn key(&self, time: u32, level: usize, channel: usize, coord: [u32; 3]) -> Key {
        Key {
            identity: self.identity.unwrap_or(0),
            level: u32::try_from(level).expect("a pyramid holds fewer than 4G levels"),
            time,
            channel: u32::try_from(channel).expect("a pyramid holds fewer than 4G channels"),
            coord,
        }
    }

    fn source_stamp(
        &self,
        time: u32,
        level: usize,
        channel: usize,
        ranges: &[Range<u32>; 3],
    ) -> Result<Option<u64>, Error> {
        let scale = Scale::of(&self.pyramid, level);
        let source = scale.source_ranges(ranges);
        if source.iter().any(|range| range.start >= range.end) {
            return Ok(Some(0));
        }
        match &self.inner {
            Inner::Zarr(image) => Ok(image.stamps(
                scale.source(),
                time,
                channel,
                source[0].clone(),
                source[1].clone(),
                source[2].clone(),
            )?),
            Inner::Memory(_) => Ok(None),
            Inner::Tiff(_) | Inner::Czi(_) | Inner::Readout(_) => Ok(Some(0)),
        }
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
        let basis = level.basis();
        for axis in 0..3 {
            for value in basis.column(axis) {
                bytes.extend_from_slice(&value.to_bits().to_le_bytes());
            }
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
        bytes.push(channel.format().code());
        bytes.push(channel.semantics().code());
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
        return is_zarr(path);
    }
    if let Some(prefix) = prefix(path, 10)
        && recognised(&prefix)
    {
        return true;
    }
    grana_readout::has_extension(path) && grana_readout::is_supported(path)
}

fn recognised(prefix: &[u8]) -> bool {
    prefix.starts_with(b"ZISRAWFILE")
        || matches!(
            prefix.get(..4),
            Some(b"II*\0" | b"MM\0*" | b"II+\0" | b"MM\0+")
        )
}

fn prefix(path: &Path, limit: usize) -> Option<Vec<u8>> {
    let mut file = File::open(path).ok()?;
    let mut bytes = vec![0u8; limit];
    let mut filled = 0;
    while filled < limit {
        match file.read(&mut bytes[filled..]) {
            Ok(0) => break,
            Ok(read) => filled += read,
            Err(_) => return None,
        }
    }
    bytes.truncate(filled);
    Some(bytes)
}

pub fn series_count(path: &Path) -> Result<usize, Error> {
    if path.is_dir() {
        return series_count_at(&grana_zarr::Root::local(path)?);
    }
    if is_czi(path)? {
        return Ok(grana_czi::scene_count(path)?);
    }
    if grana_readout::is_supported(path) {
        return Ok(grana_readout::scene_count(path)?);
    }
    if is_tiff(path)? {
        return Ok(grana_tiff::series_count(path)?);
    }
    Ok(1)
}

fn series_count_at(root: &grana_zarr::Root) -> Result<usize, Error> {
    if let Some(plate) = plate_of_root(root)? {
        return Ok(plate.len().max(1));
    }
    Ok(collection_of_root(root)?.map_or(1, |collection| collection.series().len().max(1)))
}

pub fn series_names(path: &Path) -> Result<Vec<Option<String>>, Error> {
    if path.is_dir() {
        return series_names_at(&grana_zarr::Root::local(path)?);
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
    if is_tiff(path)? {
        return Ok(grana_tiff::series_names(path)?);
    }
    Ok(Vec::new())
}

fn series_names_at(root: &grana_zarr::Root) -> Result<Vec<Option<String>>, Error> {
    if let Some(plate) = plate_of_root(root)? {
        return Ok(plate
            .fields()
            .iter()
            .map(|field| Some(field.name().to_string()))
            .collect());
    }
    Ok(collection_of_root(root)?
        .map(|collection| {
            collection
                .series()
                .iter()
                .map(|series| series.name().map(str::to_string))
                .collect()
        })
        .unwrap_or_default())
}

pub fn series_group(path: &Path, series: usize) -> Result<Option<String>, Error> {
    if !path.is_dir() {
        return Ok(None);
    }
    series_group_at(&grana_zarr::Root::local(path)?, series)
}

pub fn remote_series_group(url: &str, series: usize) -> Result<Option<String>, Error> {
    series_group_at(&grana_zarr::Root::remote(url)?, series)
}

fn series_group_at(root: &grana_zarr::Root, series: usize) -> Result<Option<String>, Error> {
    if let Some(plate) = plate_of_root(root)? {
        let field = plate.field(series).ok_or_else(|| Error::Unsupported {
            path: PathBuf::from(root.display()),
            detail: format!(
                "the plate holds {} fields of view and field {series} was requested",
                plate.len()
            ),
        })?;
        return Ok(Some(field.path().to_string()));
    }
    let Some(collection) = collection_of_root(root)? else {
        return Ok(None);
    };
    let entry = collection
        .series_at(series)
        .ok_or_else(|| Error::Unsupported {
            path: PathBuf::from(root.display()),
            detail: format!(
                "the collection holds {} series and series {series} was requested",
                collection.series().len()
            ),
        })?;
    Ok(Some(entry.path().to_string()))
}

pub fn plate(source: &Source) -> Result<Option<Plate>, Error> {
    match source {
        Source::Memory(_) => Ok(None),
        Source::File { path, .. } => {
            if !path.is_dir() {
                return Ok(None);
            }
            plate_of_root(&grana_zarr::Root::local(path)?)
        }
        Source::Remote { url, .. } => plate_of_root(&grana_zarr::Root::remote(url)?),
    }
}

pub fn plate_at(path: &std::path::Path) -> Result<Option<Plate>, Error> {
    if !path.is_dir() || !is_zarr(path) {
        return Ok(None);
    }
    plate_of_root(&grana_zarr::Root::local(path)?)
}

fn plate_of_root(root: &grana_zarr::Root) -> Result<Option<Plate>, Error> {
    if grana_zarr::group_kind(root)? != grana_zarr::GroupKind::Plate {
        return Ok(None);
    }
    let plate = grana_zarr::open_plate(root)?;
    if plate.is_empty() {
        return Err(Error::Unsupported {
            path: PathBuf::from(root.display()),
            detail: "the OME-Zarr plate holds no field of view".to_string(),
        });
    }
    Ok(Some(plate))
}

fn collection_of_root(root: &grana_zarr::Root) -> Result<Option<grana_zarr::Collection>, Error> {
    Ok(grana_zarr::collection(root)?)
}

pub fn is_remote(location: &str) -> bool {
    grana_zarr::Root::remote(location).is_ok()
}

pub fn resolve_series(source: &Source) -> Result<Source, Error> {
    match source {
        Source::Memory(_) => Ok(source.clone()),
        Source::File {
            path,
            series,
            group,
        } => match group {
            Some(_) => Ok(source.clone()),
            None => Ok(match series_group(path, *series)? {
                Some(group) => Source::file_group(path.clone(), *series, group),
                None => source.clone(),
            }),
        },
        Source::Remote { url, series, group } => match group {
            Some(_) => Ok(source.clone()),
            None => Ok(match remote_series_group(url, *series)? {
                Some(group) => Source::remote_group(url.clone(), *series, group),
                None => source.clone(),
            }),
        },
    }
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
            let root = grana_zarr::Root::local(path)?;
            let (image, series) = open_zarr(root, path, series, group)?;
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
        if is_tiff(path)? {
            let image = grana_tiff::Image::open(path, series)?;
            let series = image
                .series()
                .iter()
                .map(|series| {
                    Series::new(
                        series.extent(),
                        series.channels(),
                        series.frames(),
                        series.name().map(str::to_string),
                    )
                })
                .collect();
            return Ok((Self::Tiff(image), series));
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
        Err(Error::Unsupported {
            path: path.to_path_buf(),
            detail: "the file stores no image grana can read".to_string(),
        })
    }

    fn open_remote(
        url: &str,
        series: usize,
        group: Option<&str>,
    ) -> Result<(Self, Vec<Series>), Error> {
        let root = grana_zarr::Root::remote(url)?;
        let (image, series) = open_zarr(root, Path::new(url), series, group)?;
        Ok((Self::Zarr(image), series))
    }
}

fn open_zarr(
    root: grana_zarr::Root,
    location: &Path,
    series: usize,
    group: Option<&str>,
) -> Result<(grana_zarr::Image, Vec<Series>), Error> {
    let plate = grana_zarr::group_kind(&root)? == grana_zarr::GroupKind::Plate;
    if let Some(group) = group {
        let image = grana_zarr::Image::open_root(root.clone(), group)?;
        let listed = if plate {
            vec![Series::of(image.pyramid())]
        } else if let Some(collection) = collection_of_root(&root)? {
            collection_series(&root, &collection, series, &image)?
        } else {
            single_series(image.pyramid(), location, series)?
        };
        return Ok((image, listed));
    }
    if plate {
        return Err(Error::Unsupported {
            path: location.to_path_buf(),
            detail: "the OME-Zarr plate lists its fields of view as series".to_string(),
        });
    }
    let Some(collection) = collection_of_root(&root)? else {
        let image = grana_zarr::Image::open_root(root, "/")?;
        let listed = single_series(image.pyramid(), location, series)?;
        return Ok((image, listed));
    };
    let entry = collection
        .series_at(series)
        .ok_or_else(|| Error::Unsupported {
            path: location.to_path_buf(),
            detail: format!(
                "the collection holds {} series and series {series} was requested",
                collection.series().len()
            ),
        })?;
    let image = grana_zarr::Image::open_root(root.clone(), entry.path())?;
    let listed = collection_series(&root, &collection, series, &image)?;
    Ok((image, listed))
}

fn collection_series(
    root: &grana_zarr::Root,
    collection: &grana_zarr::Collection,
    selected: usize,
    opened: &grana_zarr::Image,
) -> Result<Vec<Series>, Error> {
    let mut listed = Vec::with_capacity(collection.series().len());
    for (index, entry) in collection.series().iter().enumerate() {
        let name = entry.name().map(str::to_string);
        if index == selected {
            let pyramid = opened.pyramid();
            listed.push(Series::new(
                pyramid.level(0).extent(),
                pyramid.channel_count(),
                pyramid.frame_count(),
                name,
            ));
            continue;
        }
        let outline = match entry.outline() {
            Some(outline) => outline,
            None => grana_zarr::Image::outline(root, entry.path())?,
        };
        listed.push(Series::new(
            outline.extent(),
            outline.channels() as usize,
            outline.frames(),
            name,
        ));
    }
    Ok(listed)
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
