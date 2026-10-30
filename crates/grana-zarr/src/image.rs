use std::path::{Path, PathBuf};
use std::sync::Arc;

use grana_volume::{Channel, Extent, Intensity, Spacing, Volume, VoxelData};
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

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LevelInfo {
    pub extent: Extent,
    pub spacing: Spacing,
}

#[derive(Clone, Debug)]
pub struct ChannelInfo {
    pub name: String,
    pub tint: [f32; 3],
    pub window: Option<[f32; 2]>,
}

struct Level {
    array_path: String,
    info: LevelInfo,
    layout: Layout,
}

pub struct Image {
    path: PathBuf,
    store: ReadableWritableListableStorage,
    name: String,
    levels: Vec<Level>,
    channels: Vec<ChannelInfo>,
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
        for dataset in &metadata.datasets {
            let array_path = array_path(&dataset.path);
            let array = open_array(&store, &array_path)?;
            ensure_supported(path, &array)?;
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
            levels.push(Level {
                array_path,
                info: LevelInfo {
                    extent: layout.extent,
                    spacing: layout.spacing,
                },
                layout,
            });
        }
        let channel_count = channel_count.expect("a multiscale image has at least one dataset");
        let channels = channels(path, &metadata.channels, channel_count)?;
        let name = metadata
            .name
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| dataset_name(path));
        Ok(Self {
            path: path.to_path_buf(),
            store,
            name,
            levels,
            channels,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn channels(&self) -> &[ChannelInfo] {
        &self.channels
    }

    pub fn levels(&self) -> impl ExactSizeIterator<Item = LevelInfo> + '_ {
        self.levels.iter().map(|level| level.info)
    }

    pub fn read(&self, level: usize) -> Result<Volume, Error> {
        let Some(level) = self.levels.get(level) else {
            return Err(Error::Unsupported {
                path: self.path.clone(),
                detail: "the requested resolution level does not exist".to_string(),
            });
        };
        let array = open_array(&self.store, &level.array_path)?;
        let mut channels = Vec::with_capacity(self.channels.len());
        for (index, info) in self.channels.iter().enumerate() {
            let subset = ArraySubset::new_with_ranges(&level.layout.ranges(index as u64));
            let data = retrieve(&array, &subset, &level.array_path)?;
            if data.voxel_count() as u64 != level.info.extent.voxel_count() {
                return Err(Error::Read {
                    array: level.array_path.clone(),
                    detail: "the decoded array has an unexpected voxel count".to_string(),
                });
            }
            let channel = Channel::new(&info.name, info.tint, level.info.extent, data);
            let channel = match info.window {
                Some([start, end]) if end > start => {
                    channel.with_window(Intensity::new(start, end))
                }
                _ => channel,
            };
            channels.push(channel);
        }
        Ok(Volume::new(level.info.extent, level.info.spacing, channels))
    }
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

fn ensure_supported(
    path: &Path,
    array: &Array<dyn ReadableWritableListableStorageTraits>,
) -> Result<(), Error> {
    let data_type = array.data_type();
    if data_type == &uint8() || data_type == &uint16() || data_type == &float32() {
        Ok(())
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
    Ok((0..count)
        .map(|index| {
            let metadata = metadata.get(index as usize);
            let name = metadata
                .and_then(|metadata| metadata.label.clone())
                .filter(|label| !label.is_empty())
                .unwrap_or_else(|| format!("Channel {}", index + 1));
            let tint = metadata
                .map(|metadata| metadata.tint)
                .unwrap_or(PALETTE[index as usize % PALETTE.len()]);
            let window = metadata.and_then(|metadata| metadata.window);
            ChannelInfo { name, tint, window }
        })
        .collect())
}

fn dataset_name(path: &Path) -> String {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return "OME-Zarr".to_string();
    };
    let name = name.strip_suffix(".zarr").unwrap_or(name);
    let name = name.strip_suffix(".ome").unwrap_or(name);
    name.to_string()
}
