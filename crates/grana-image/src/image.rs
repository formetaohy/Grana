use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;

use grana_volume::{
    BRICK_SIDE, BrickCoord, BrickData, BrickGrid, ChannelInfo, Extent, NoData, Pyramid, VoxelData,
    pad_brick,
};
use grana_warp::{Warp, Warps};
use rayon::prelude::*;

use crate::Error;
use crate::align;
use crate::reader::{Format, Reader};
use crate::slice::{Store, Vault, default_root, mix, modified, stamp};
use crate::{Series, Source};

const RESOLVED_LIMIT: usize = 8;

const FNV_BASE: u64 = 0xcbf2_9ce4_8422_2325;

pub struct Image {
    pyramid: Pyramid,
    series: Vec<Series>,
    selected: usize,
    vault: Vault,
}

impl Image {
    pub fn open(source: &Source) -> Result<Self, Error> {
        Self::open_with_cache(source, default_root())
    }

    pub fn open_with_cache(source: &Source, cache: Option<PathBuf>) -> Result<Self, Error> {
        let (reader, series) = match source {
            Source::Memory(volume) => (
                Reader::Memory(Arc::clone(volume)),
                vec![Series::of(volume.pyramid())],
            ),
            Source::File {
                path,
                series,
                group,
            } => Reader::open_file(path, *series, group.as_deref())?,
            Source::Remote { url, series, group } => {
                Reader::open_remote(url, *series, group.as_deref())?
            }
        };
        let pyramid = extended(reader.pyramid());
        let identity = identity(source, &geometry(&pyramid), reader.digest());
        let namespace = identity.unwrap_or_else(anonymous);
        let store = match (source, identity) {
            (Source::Memory(_), _) | (_, None) => None,
            (Source::File { .. } | Source::Remote { .. }, Some(_)) => {
                cache.as_deref().and_then(Store::open)
            }
        };
        Ok(Self {
            pyramid,
            series,
            selected: source.series(),
            vault: Vault::new(reader, namespace, store),
        })
    }

    pub fn series(&self) -> &[Series] {
        &self.series
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn name(&self) -> &str {
        self.vault.reader().name()
    }

    pub fn transfer(&self) -> Option<Arc<grana_zarr::Transfer>> {
        self.vault.reader().transfer()
    }

    pub fn pyramid(&self) -> &Pyramid {
        &self.pyramid
    }

    pub fn decode(&self) -> Option<grana_zarr::Decode> {
        self.vault.reader().decode()
    }

    pub fn labels(&self) -> Result<grana_zarr::Labels, Error> {
        self.vault.reader().labels()
    }

    pub fn format(&self) -> Option<Format> {
        self.vault.reader().format()
    }

    pub fn family(&self) -> Option<grana_readout::Family> {
        self.vault.reader().family()
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
        self.window(time, level, channel, ranges, warp)
    }

    pub fn fill(&self, channel: usize) -> Result<NoData, Error> {
        self.pyramid
            .channels()
            .get(channel)
            .map(ChannelInfo::fill)
            .ok_or_else(|| Error::Resolved {
                detail: "the requested channel does not exist".to_string(),
            })
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
        if let Some(fill) = self.empty(time, level, coord, warps)? {
            return Ok(BrickData::new(
                (0..self.pyramid.channel_count())
                    .map(|_| grana_volume::blank_brick(fill))
                    .collect(),
            ));
        }
        let bricks = (0..self.pyramid.channel_count())
            .into_par_iter()
            .map(|channel| self.brick(time, level, channel, coord, warps.get(channel)))
            .collect::<Result<Vec<_>, Error>>()?;
        Ok(BrickData::new(bricks))
    }

    pub fn empty(
        &self,
        time: u32,
        level: usize,
        coord: BrickCoord,
        warps: &Warps,
    ) -> Result<Option<NoData>, Error> {
        if level >= self.pyramid.stored() || !warps.identity() {
            return Ok(None);
        }
        let Some(fill) = self.vault.reader().empty(time, level, coord)? else {
            return Ok(None);
        };
        let uniform = self
            .pyramid
            .channels()
            .iter()
            .all(|channel| channel.format() == fill.format());
        Ok(uniform.then_some(fill))
    }

    fn brick(
        &self,
        time: u32,
        level: usize,
        channel: usize,
        coord: BrickCoord,
        warp: &Warp,
    ) -> Result<VoxelData, Error> {
        let extent = self.level_extent(level)?;
        let grid = BrickGrid::new(extent);
        let spatial = grid.padded_range(coord);
        let origin = [spatial[0].start, spatial[1].start, spatial[2].start];
        let covered = covered_extent(&spatial);
        let data = self.window(time, level, channel, &spatial, warp)?;
        Ok(pad_brick(
            &data,
            origin,
            covered,
            grid.origin(coord),
            extent,
        ))
    }

    fn window(
        &self,
        time: u32,
        level: usize,
        channel: usize,
        ranges: &[Range<u32>; 3],
        warp: &Warp,
    ) -> Result<VoxelData, Error> {
        let fill = self.fill(channel)?;
        if warp.identity() {
            return self
                .vault
                .window(&self.pyramid, time, level, channel, ranges, fill);
        }
        self.aligned(time, level, channel, ranges, warp, fill)
    }

    fn aligned(
        &self,
        time: u32,
        level: usize,
        channel: usize,
        ranges: &[Range<u32>; 3],
        warp: &Warp,
        fill: NoData,
    ) -> Result<VoxelData, Error> {
        let extent = self.level_extent(level)?;
        let info = self.pyramid.level(level);
        let semantics = self.pyramid.channels()[channel].semantics();
        let limits = [extent.x(), extent.y(), extent.z()];
        let plan = align::plan(ranges, info, warp, limits);
        if plan.source().iter().any(|range| range.start >= range.end) {
            return Ok(align::filled(fill, ranges));
        }
        let data = self
            .vault
            .window(&self.pyramid, time, level, channel, plan.source(), fill)?;
        Ok(align::resample(&data, &plan, ranges, semantics, fill))
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

fn extended(pyramid: &Pyramid) -> Pyramid {
    let mut levels = pyramid.levels().to_vec();
    let stored = levels.len();
    while levels.len() - stored < RESOLVED_LIMIT {
        let last = *levels.last().expect("a source holds at least one level");
        if last.extent().max_dimension() <= BRICK_SIDE {
            break;
        }
        levels.push(last.coarser());
    }
    Pyramid::placed(
        levels,
        stored,
        pyramid.channels().to_vec(),
        pyramid.frame_count(),
        pyramid.frame_interval(),
        pyramid.placement(),
        pyramid.calibration(),
    )
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
    bytes.extend_from_slice(&stamp(FNV_BASE).to_le_bytes());
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

fn identity(source: &Source, geometry: &[u8], dataset: Option<u64>) -> Option<u64> {
    let mut hash = mix(FNV_BASE, geometry);
    match source {
        Source::Memory(_) => None,
        Source::File {
            path,
            series,
            group,
        } => {
            let resolved = std::fs::canonicalize(path).unwrap_or_else(|_| path.clone());
            hash = mix(hash, resolved.to_string_lossy().as_bytes());
            hash = mix(hash, &(*series as u64).to_le_bytes());
            if let Some(group) = group {
                hash = mix(hash, group.as_bytes());
            }
            match dataset {
                Some(digest) => Some(mix(hash, &digest.to_le_bytes())),
                None => {
                    let metadata = std::fs::metadata(path).ok()?;
                    if metadata.is_dir() {
                        return None;
                    }
                    Some(mix(
                        mix(hash, &metadata.len().to_le_bytes()),
                        &modified(&metadata).to_le_bytes(),
                    ))
                }
            }
        }
        Source::Remote { url, series, group } => {
            let mut hash = mix(hash, url.as_bytes());
            hash = mix(hash, &(*series as u64).to_le_bytes());
            if let Some(group) = group {
                hash = mix(hash, group.as_bytes());
            }
            Some(mix(hash, &dataset?.to_le_bytes()))
        }
    }
}

fn anonymous() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static ORDINAL: AtomicU64 = AtomicU64::new(0);
    let ordinal = ORDINAL.fetch_add(1, Ordering::Relaxed);
    mix(mix(FNV_BASE, b"grana-anonymous"), &ordinal.to_le_bytes())
}
