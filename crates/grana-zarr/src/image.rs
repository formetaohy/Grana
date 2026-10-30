use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use grana_volume::{
    BrickCoord, BrickData, BrickGrid, ChannelInfo, Extent, Intensity, Level, Pyramid, VoxelData,
    VoxelFormat, pad_brick,
};
use zarrs::array::data_type::{float32, uint8, uint16};
use zarrs::array::{Array, ArraySubset};
use zarrs::filesystem::FilesystemStore;
use zarrs::group::Group;
use zarrs::storage::{ReadableWritableListableStorage, ReadableWritableListableStorageTraits};

use crate::Error;
use crate::axes::{self, Layout};
use crate::metadata::{self, ChannelMetadata};

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

struct LevelArray {
    path: String,
    array: Array<dyn ReadableWritableListableStorageTraits>,
    layout: Layout,
}

pub struct Image {
    path: PathBuf,
    name: String,
    levels: Vec<LevelArray>,
    pyramid: Pyramid,
}

impl Image {
    pub fn open(path: &Path) -> Result<Self, Error> {
        let store: ReadableWritableListableStorage =
            Arc::new(FilesystemStore::new(path).map_err(|error| Error::Open {
                path: path.to_path_buf(),
                detail: error.to_string(),
            })?);
        let group = Group::open(store.clone(), "/").map_err(|error| Error::Open {
            path: path.to_path_buf(),
            detail: error.to_string(),
        })?;
        let metadata = metadata::parse(path, group.attributes())?;
        let mut levels = Vec::with_capacity(metadata.datasets.len());
        let mut channel_count = None;
        let mut format = None;
        for dataset in &metadata.datasets {
            let array_path = array_path(&dataset.path);
            let array = open_array(&store, &array_path)?;
            let level_format = supported_format(path, &array)?;
            match format {
                None => format = Some(level_format),
                Some(known) if known != level_format => {
                    return Err(Error::Unsupported {
                        path: path.to_path_buf(),
                        detail: "the resolution levels disagree on the voxel data type".to_string(),
                    });
                }
                Some(_) => {}
            }
            let layout =
                axes::layout(&metadata.axes, array.shape(), &dataset.scale).map_err(|detail| {
                    Error::Unsupported {
                        path: path.to_path_buf(),
                        detail,
                    }
                })?;
            let count = u32::try_from(layout.channel_count()).map_err(|_| Error::Unsupported {
                path: path.to_path_buf(),
                detail: "the channel axis is too long".to_string(),
            })?;
            match channel_count {
                None => channel_count = Some(count),
                Some(known) if known != count => {
                    return Err(Error::Unsupported {
                        path: path.to_path_buf(),
                        detail: "the resolution levels disagree on the channel count".to_string(),
                    });
                }
                Some(_) => {}
            }
            levels.push(LevelArray {
                path: array_path,
                array,
                layout,
            });
        }
        let channel_count = channel_count.expect("a multiscale image has at least one dataset");
        if !levels_ordered(&levels) {
            return Err(Error::Unsupported {
                path: path.to_path_buf(),
                detail: "the multiscale levels are not ordered from fine to coarse".to_string(),
            });
        }
        let format = format.expect("a multiscale image has at least one dataset");
        let channels = channels(path, &metadata.channels, channel_count, format, &levels)?;
        let name = metadata
            .name
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| dataset_name(path));
        let level_metadata = levels
            .iter()
            .map(|level| Level::new(level.layout.extent, level.layout.spacing))
            .collect();
        Ok(Self {
            path: path.to_path_buf(),
            name,
            levels,
            pyramid: Pyramid::new(level_metadata, channels),
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn pyramid(&self) -> &Pyramid {
        &self.pyramid
    }

    pub fn read_brick(&self, level: usize, coord: BrickCoord) -> Result<BrickData, Error> {
        let channels = (0..self.pyramid.channel_count())
            .map(|channel| self.read_channel_brick(level, channel, coord))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(BrickData::new(channels))
    }

    pub fn read_channel_brick(
        &self,
        level: usize,
        channel: usize,
        coord: BrickCoord,
    ) -> Result<VoxelData, Error> {
        let Some(level_array) = self.levels.get(level) else {
            return Err(Error::Unsupported {
                path: self.path.clone(),
                detail: "the requested resolution level does not exist".to_string(),
            });
        };
        if channel >= self.pyramid.channel_count() {
            return Err(Error::Unsupported {
                path: self.path.clone(),
                detail: "the requested channel does not exist".to_string(),
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
        level: usize,
        channel: usize,
        x: Range<u32>,
        y: Range<u32>,
        z: Range<u32>,
    ) -> Result<VoxelData, Error> {
        let Some(level) = self.levels.get(level) else {
            return Err(Error::Region {
                array: self.path.display().to_string(),
                detail: "the requested resolution level does not exist".to_string(),
            });
        };
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
        let ranges = level
            .layout
            .ranges(channel as u64, x.clone(), y.clone(), z.clone());
        let subset = ArraySubset::new_with_ranges(&ranges);
        let data = retrieve(&level.array, &subset, &level.path)?;
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

fn array_path(path: &str) -> String {
    if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    }
}

fn open_array(
    store: &ReadableWritableListableStorage,
    path: &str,
) -> Result<Array<dyn ReadableWritableListableStorageTraits>, Error> {
    Array::open(store.clone(), path).map_err(|error| Error::Open {
        path: PathBuf::from(path),
        detail: error.to_string(),
    })
}

fn supported_format(
    path: &Path,
    array: &Array<dyn ReadableWritableListableStorageTraits>,
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
            path: path.to_path_buf(),
            detail: format!("the voxel data type `{data_type}` is not supported"),
        })
    }
}

fn retrieve(
    array: &Array<dyn ReadableWritableListableStorageTraits>,
    subset: &ArraySubset,
    path: &str,
) -> Result<VoxelData, Error> {
    let mut read = |error: zarrs::array::ArrayError| Error::Read {
        array: path.to_string(),
        detail: error.to_string(),
    };
    let data_type = array.data_type();
    if data_type == &uint8() {
        Ok(VoxelData::U8(
            array
                .retrieve_array_subset::<Vec<u8>>(subset)
                .map_err(&mut read)?,
        ))
    } else if data_type == &uint16() {
        Ok(VoxelData::U16(
            array
                .retrieve_array_subset::<Vec<u16>>(subset)
                .map_err(&mut read)?,
        ))
    } else {
        Ok(VoxelData::F32(
            array
                .retrieve_array_subset::<Vec<f32>>(subset)
                .map_err(&mut read)?,
        ))
    }
}

fn channels(
    path: &Path,
    metadata: &[ChannelMetadata],
    count: u32,
    format: VoxelFormat,
    levels: &[LevelArray],
) -> Result<Vec<ChannelInfo>, Error> {
    if !metadata.is_empty() && metadata.len() != count as usize {
        return Err(Error::Metadata {
            path: path.to_path_buf(),
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
                sample_range(levels, index as usize)?
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

fn sample_range(levels: &[LevelArray], channel: usize) -> Result<Option<Intensity>, Error> {
    let level = levels.last().expect("a multiscale image has a level");
    let extent = level.layout.extent;
    let probe = |limit: u32| {
        let side = limit.min(RANGE_PROBE_SIDE);
        let start = (limit - side) / 2;
        start..start + side
    };
    let spatial = if extent.voxel_count() <= RANGE_SAMPLE_VOXELS {
        (0..extent.x(), 0..extent.y(), 0..extent.z())
    } else {
        (probe(extent.x()), probe(extent.y()), probe(extent.z()))
    };
    let ranges = level
        .layout
        .ranges(channel as u64, spatial.0, spatial.1, spatial.2);
    let subset = ArraySubset::new_with_ranges(&ranges);
    let data = retrieve(&level.array, &subset, &level.path)?;
    let range = data.range();
    if range.span() > 0.0 {
        Ok(Some(range))
    } else {
        Ok(None)
    }
}

fn valid(range: [f32; 2]) -> Option<Intensity> {
    (range[0].is_finite() && range[1].is_finite() && range[0] < range[1])
        .then(|| Intensity::new(range[0], range[1]))
}

fn dataset_name(path: &Path) -> String {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return "OME-Zarr".to_string();
    };
    let name = name.strip_suffix(".zarr").unwrap_or(name);
    let name = name.strip_suffix(".ome").unwrap_or(name);
    name.to_string()
}
