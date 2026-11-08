use std::ops::Range;
use std::path::Path;
use std::sync::Arc;

use grana_volume::{
    BrickCoord, BrickData, BrickGrid, ChannelInfo, Extent, Intensity, Pyramid, VoxelData,
    VoxelFormat, pad_brick,
};
use zarrs::array::chunk_cache::{ChunkCache, ChunkCacheLruSizeLimit, ChunkCacheTypeDecoded};
use zarrs::array::data_type::{float32, uint8, uint16};
use zarrs::array::{Array, ArraySubset, CodecOptions};
use zarrs::group::Group;
use zarrs::storage::ReadableStorageTraits;

use rayon::prelude::*;

use crate::Error;
use crate::axes::{self, Layout};
use crate::metadata::{self, ChannelMetadata};
use crate::root::Root;

const PALETTE: [[f32; 3]; 8] = [
    [0.25, 0.55, 1.00],
    [0.20, 0.95, 0.35],
    [1.00, 0.25, 0.55],
    [1.00, 0.72, 0.20],
    [0.60, 0.40, 1.00],
    [0.20, 0.90, 0.90],
    [1.00, 0.45, 0.20],
    [0.85, 0.85, 0.30],
];

const RANGE_SAMPLE_VOXELS: u64 = 4 << 20;
const RANGE_PROBE_SIDE: u32 = 32;
const RANGE_SAMPLE_FRAMES: u32 = 4;
const CHUNK_CACHE_BYTES: u64 = 128 << 20;
const CHUNK_CACHE_MINIMUM: u64 = 8 << 20;

type Cache = ChunkCacheLruSizeLimit<ChunkCacheTypeDecoded>;

struct LevelArray {
    path: String,
    cache: Cache,
    layout: Layout,
}

pub struct Image {
    root: Root,
    group: String,
    name: String,
    version: String,
    minor: u32,
    levels: Vec<LevelArray>,
    pyramid: Pyramid,
    axes: Vec<metadata::Axis>,
    datasets: Vec<metadata::Dataset>,
}

impl Image {
    pub fn open(path: &Path) -> Result<Self, Error> {
        Self::open_root(Root::local(path)?, "/")
    }

    pub fn open_group(path: &Path, group: &str) -> Result<Self, Error> {
        Self::open_root(Root::local(path)?, group)
    }

    pub fn open_root(root: Root, group: &str) -> Result<Self, Error> {
        let tag = root.tag(group);
        let opened = Group::open(root.listable(), group).map_err(|error| Error::Open {
            path: tag.clone(),
            detail: error.to_string(),
        })?;
        let metadata = metadata::parse(&tag, opened.attributes())?;
        let readable: Arc<dyn ReadableStorageTraits> = root.readable();
        let cache_bytes =
            (CHUNK_CACHE_BYTES / metadata.datasets.len() as u64).max(CHUNK_CACHE_MINIMUM);
        let mut levels = Vec::with_capacity(metadata.datasets.len());
        let mut channel_count = None;
        let mut frames = None;
        let mut interval = None;
        let mut format = None;
        for dataset in &metadata.datasets {
            let array_path = crate::hcs::join(group, &dataset.path);
            let array = Arc::new(open_array(readable.clone(), &array_path)?);
            let level_format = supported_format(&tag, &array)?;
            match format {
                None => format = Some(level_format),
                Some(known) if known != level_format => {
                    return Err(Error::Unsupported {
                        path: tag.clone(),
                        detail: "the resolution levels disagree on the voxel data type".to_string(),
                    });
                }
                Some(_) => {}
            }
            let layout = axes::layout(
                &metadata.axes,
                array.shape(),
                &dataset.scale,
                &dataset.translation,
            )
            .map_err(|detail| Error::Unsupported {
                path: tag.clone(),
                detail,
            })?;
            let count = u32::try_from(layout.channel_count()).map_err(|_| Error::Unsupported {
                path: tag.clone(),
                detail: "the channel axis is too long".to_string(),
            })?;
            match channel_count {
                None => channel_count = Some(count),
                Some(known) if known != count => {
                    return Err(Error::Unsupported {
                        path: tag.clone(),
                        detail: "the resolution levels disagree on the channel count".to_string(),
                    });
                }
                Some(_) => {}
            }
            match frames {
                None => frames = Some(layout.frame_count()),
                Some(known) if known != layout.frame_count() => {
                    return Err(Error::Unsupported {
                        path: tag.clone(),
                        detail: "the resolution levels disagree on the frame count".to_string(),
                    });
                }
                Some(_) => {}
            }
            match interval {
                None => interval = Some(layout.frame_interval()),
                Some(known) if known != layout.frame_interval() => {
                    return Err(Error::Unsupported {
                        path: tag.clone(),
                        detail: "the resolution levels disagree on the frame interval".to_string(),
                    });
                }
                Some(_) => {}
            }
            levels.push(LevelArray {
                path: array_path,
                cache: Cache::new(array, cache_bytes),
                layout,
            });
        }
        let channel_count = channel_count.expect("a multiscale image has at least one dataset");
        let frames = frames.expect("a multiscale image has at least one dataset");
        let interval = interval.expect("a multiscale image has at least one dataset");
        if !levels_ordered(&levels) {
            return Err(Error::Unsupported {
                path: tag,
                detail: "the multiscale levels are not ordered from fine to coarse".to_string(),
            });
        }
        let format = format.expect("a multiscale image has at least one dataset");
        let channels = channels(
            &root.tag(group),
            &metadata.channels,
            channel_count,
            format,
            &levels,
            frames,
        )?;
        let name = metadata
            .name
            .clone()
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| root.name_at(group));
        let (level_metadata, placement) = crate::frame::levels(
            &levels
                .iter()
                .map(|level| level.layout.clone())
                .collect::<Vec<_>>(),
        );
        let stored = level_metadata.len();
        Ok(Self {
            root,
            group: group.to_string(),
            name,
            version: metadata.version,
            minor: metadata.minor,
            levels,
            pyramid: Pyramid::placed(
                level_metadata,
                stored,
                channels,
                frames,
                interval,
                placement,
            ),
            axes: metadata.axes,
            datasets: metadata.datasets,
        })
    }

    pub(crate) fn version(&self) -> &str {
        &self.version
    }

    pub(crate) fn minor(&self) -> u32 {
        self.minor
    }

    pub(crate) fn modern(&self) -> bool {
        self.minor >= 6
    }

    pub(crate) fn axes(&self) -> &[metadata::Axis] {
        &self.axes
    }

    pub(crate) fn datasets(&self) -> &[metadata::Dataset] {
        &self.datasets
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn revision(&self) -> Option<String> {
        self.root.revision()
    }

    pub fn labels(&self) -> Result<crate::Labels, Error> {
        crate::label::read_at(&self.root, &self.group)
    }

    pub fn pyramid(&self) -> &Pyramid {
        &self.pyramid
    }

    pub fn read_brick(
        &self,
        time: u32,
        level: usize,
        coord: BrickCoord,
    ) -> Result<BrickData, Error> {
        let channels = (0..self.pyramid.channel_count())
            .into_par_iter()
            .map(|channel| self.read_channel_brick(time, level, channel, coord))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(BrickData::new(channels))
    }

    pub fn read_channel_brick(
        &self,
        time: u32,
        level: usize,
        channel: usize,
        coord: BrickCoord,
    ) -> Result<VoxelData, Error> {
        let Some(level_array) = self.levels.get(level) else {
            return Err(Error::Unsupported {
                path: self.root.tag(&self.group),
                detail: "the requested resolution level does not exist".to_string(),
            });
        };
        if channel >= self.pyramid.channel_count() {
            return Err(Error::Unsupported {
                path: self.root.tag(&self.group),
                detail: "the requested channel does not exist".to_string(),
            });
        }
        if time >= self.pyramid.frame_count() {
            return Err(Error::Unsupported {
                path: self.root.tag(&self.group),
                detail: "the requested frame does not exist".to_string(),
            });
        }
        let grid = BrickGrid::new(level_array.layout.extent);
        let spatial = grid.padded_range(coord);
        let origin = [spatial[0].start, spatial[1].start, spatial[2].start];
        let extent = Extent::new(
            spatial[0].end - spatial[0].start,
            spatial[1].end - spatial[1].start,
            spatial[2].end - spatial[2].start,
        );
        let data = self.read_region(
            time,
            level,
            channel,
            spatial[0].clone(),
            spatial[1].clone(),
            spatial[2].clone(),
        )?;
        Ok(pad_brick(
            &data,
            origin,
            extent,
            grid.origin(coord),
            level_array.layout.extent,
        ))
    }

    pub fn read_region(
        &self,
        time: u32,
        level: usize,
        channel: usize,
        x: Range<u32>,
        y: Range<u32>,
        z: Range<u32>,
    ) -> Result<VoxelData, Error> {
        let Some(level) = self.levels.get(level) else {
            return Err(Error::Region {
                array: self.root.tag(&self.group),
                detail: "the requested resolution level does not exist".to_string(),
            });
        };
        if time >= self.pyramid.frame_count() {
            return Err(Error::Region {
                array: level.path.clone(),
                detail: "the requested frame does not exist".to_string(),
            });
        }
        if channel >= self.pyramid.channel_count() {
            return Err(Error::Region {
                array: level.path.clone(),
                detail: format!(
                    "the channel {channel} lies outside the {} available channels",
                    self.pyramid.channel_count()
                ),
            });
        }
        let extent = level.layout.extent;
        if x.start >= x.end
            || x.end > extent.x()
            || y.start >= y.end
            || y.end > extent.y()
            || z.start >= z.end
            || z.end > extent.z()
        {
            return Err(Error::Region {
                array: level.path.clone(),
                detail: "the region lies outside the array extent".to_string(),
            });
        }
        let ranges = level.layout.ranges(
            u64::from(time),
            channel as u64,
            x.clone(),
            y.clone(),
            z.clone(),
        );
        let subset = ArraySubset::new_with_ranges(&ranges);
        let data = retrieve(&level.cache, &subset, &level.path)?;
        let expected =
            u64::from(x.end - x.start) * u64::from(y.end - y.start) * u64::from(z.end - z.start);
        if data.voxel_count() as u64 != expected {
            return Err(Error::Read {
                array: level.path.clone(),
                detail: "the decoded region has an unexpected voxel count".to_string(),
            });
        }
        Ok(data)
    }
}

fn levels_ordered(levels: &[LevelArray]) -> bool {
    levels.windows(2).all(|pair| {
        let fine = &pair[0].layout;
        let coarse = &pair[1].layout;
        let (fine_extent, coarse_extent) = (fine.extent, coarse.extent);
        let (fine_spacing, coarse_spacing) = (fine.spacing, coarse.spacing);
        fine_extent.x() >= coarse_extent.x()
            && fine_extent.y() >= coarse_extent.y()
            && fine_extent.z() >= coarse_extent.z()
            && fine_spacing.x() <= coarse_spacing.x()
            && fine_spacing.y() <= coarse_spacing.y()
            && fine_spacing.z() <= coarse_spacing.z()
    })
}

fn open_array(
    store: Arc<dyn ReadableStorageTraits>,
    path: &str,
) -> Result<Array<dyn ReadableStorageTraits>, Error> {
    Array::open(store, path).map_err(|error| Error::Open {
        path: path.to_string(),
        detail: error.to_string(),
    })
}

fn supported_format(
    path: &str,
    array: &Array<dyn ReadableStorageTraits>,
) -> Result<VoxelFormat, Error> {
    let data_type = array.data_type();
    if data_type == &uint8() {
        Ok(VoxelFormat::U8)
    } else if data_type == &uint16() {
        Ok(VoxelFormat::U16)
    } else if data_type == &float32() {
        Ok(VoxelFormat::F32)
    } else {
        Err(Error::Unsupported {
            path: path.to_string(),
            detail: format!("the voxel data type `{data_type}` is not supported"),
        })
    }
}

fn retrieve(cache: &Cache, subset: &ArraySubset, path: &str) -> Result<VoxelData, Error> {
    let mut read = |error: zarrs::array::ArrayError| Error::Read {
        array: path.to_string(),
        detail: error.to_string(),
    };
    let options = CodecOptions::default();
    let array = cache.array();
    let data_type = array.data_type();
    if data_type == &uint8() {
        Ok(VoxelData::U8(
            cache
                .retrieve_array_subset::<Vec<u8>>(subset, &options)
                .map_err(&mut read)?,
        ))
    } else if data_type == &uint16() {
        Ok(VoxelData::U16(
            cache
                .retrieve_array_subset::<Vec<u16>>(subset, &options)
                .map_err(&mut read)?,
        ))
    } else {
        Ok(VoxelData::F32(
            cache
                .retrieve_array_subset::<Vec<f32>>(subset, &options)
                .map_err(&mut read)?,
        ))
    }
}

fn channels(
    origin: &str,
    metadata: &[ChannelMetadata],
    count: u32,
    format: VoxelFormat,
    levels: &[LevelArray],
    frames: u32,
) -> Result<Vec<ChannelInfo>, Error> {
    if !metadata.is_empty() && metadata.len() != count as usize {
        return Err(Error::Metadata {
            path: origin.to_string(),
            detail: format!(
                "the omero metadata describes {} channels but the image has {count}",
                metadata.len()
            ),
        });
    }
    (0..count)
        .map(|index| {
            let metadata = metadata.get(index as usize);
            let name = metadata
                .and_then(|metadata| metadata.label.clone())
                .filter(|label| !label.is_empty())
                .unwrap_or_else(|| format!("Channel {}", index + 1));
            let tint = metadata
                .map(|metadata| metadata.tint)
                .unwrap_or(PALETTE[index as usize % PALETTE.len()]);
            let data_range = metadata.and_then(|metadata| metadata.data).and_then(valid);
            let omero_window = metadata
                .and_then(|metadata| metadata.window)
                .and_then(valid);
            let sampled = if data_range.is_none() && omero_window.is_none() {
                sample_range(levels, index as usize, frames)?
            } else {
                None
            };
            let intensity = match format {
                VoxelFormat::U8 => Intensity::new(0.0, f32::from(u8::MAX)),
                VoxelFormat::U16 => Intensity::new(0.0, f32::from(u16::MAX)),
                VoxelFormat::F32 => data_range
                    .or(omero_window)
                    .or(sampled)
                    .unwrap_or_else(|| Intensity::new(0.0, 1.0)),
            };
            let window = omero_window
                .or(data_range)
                .or(sampled)
                .or_else(|| (intensity.span() > 0.0).then_some(intensity))
                .unwrap_or_else(|| Intensity::new(0.0, 1.0));
            Ok(ChannelInfo::new(name, tint, intensity, window, format))
        })
        .collect()
}

fn sample_range(
    levels: &[LevelArray],
    channel: usize,
    frames: u32,
) -> Result<Option<Intensity>, Error> {
    let level = levels.last().expect("a multiscale image has a level");
    let extent = level.layout.extent;
    let samples = frames.clamp(1, RANGE_SAMPLE_FRAMES);
    let budget = (RANGE_SAMPLE_VOXELS / u64::from(samples)).max(1);
    let probe = |limit: u32| {
        let side = limit.min(RANGE_PROBE_SIDE);
        let start = (limit - side) / 2;
        start..start + side
    };
    let spatial = if extent.voxel_count() <= budget {
        (0..extent.x(), 0..extent.y(), 0..extent.z())
    } else {
        (probe(extent.x()), probe(extent.y()), probe(extent.z()))
    };
    let mut low = f32::INFINITY;
    let mut high = f32::NEG_INFINITY;
    for sample in 0..samples {
        let time = u64::from(sample) * u64::from(frames) / u64::from(samples);
        let ranges = level.layout.ranges(
            time,
            channel as u64,
            spatial.0.clone(),
            spatial.1.clone(),
            spatial.2.clone(),
        );
        let subset = ArraySubset::new_with_ranges(&ranges);
        let data = retrieve(&level.cache, &subset, &level.path)?;
        let range = data.range();
        low = low.min(range.min());
        high = high.max(range.max());
    }
    if high > low {
        Ok(Some(Intensity::new(low, high)))
    } else {
        Ok(None)
    }
}

fn valid(range: [f32; 2]) -> Option<Intensity> {
    (range[0].is_finite() && range[1].is_finite() && range[0] < range[1])
        .then(|| Intensity::new(range[0], range[1]))
}
