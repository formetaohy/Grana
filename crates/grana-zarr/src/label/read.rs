use std::ops::Range;
use std::path::Path;
use std::sync::Arc;

use grana_volume::Level;
use serde_json::{Map, Value};
use zarrs::array::chunk_cache::{ChunkCache, ChunkCacheLruSizeLimit, ChunkCacheTypeDecoded};
use zarrs::array::data_type::{int8, int16, int32, uint8, uint16, uint32, uint64};
use zarrs::array::{Array, ArraySubset, CodecOptions, DataType};
use zarrs::group::Group;
use zarrs::storage::{ReadableStorageTraits, StoreKey};

use crate::axes::{self, Layout};
use crate::error::Error;
use crate::metadata;
use crate::root::Root;

type Cache = ChunkCacheLruSizeLimit<ChunkCacheTypeDecoded>;

const CACHE_BYTES: u64 = 64 << 20;
const CACHE_MINIMUM: u64 = 4 << 20;
const MARKERS: [&str; 3] = ["zarr.json", ".zgroup", ".zattrs"];

pub struct Labels {
    images: Vec<Label>,
}

impl Labels {
    pub fn empty() -> Self {
        Self { images: Vec::new() }
    }

    pub fn images(&self) -> &[Label] {
        &self.images
    }

    pub fn get(&self, name: &str) -> Option<&Label> {
        self.images.iter().find(|label| label.name() == name)
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.images.iter().map(Label::name)
    }

    pub fn is_empty(&self) -> bool {
        self.images.is_empty()
    }
}

#[derive(Clone, Copy)]
enum Kind {
    U8,
    U16,
    U32,
    U64,
    I8,
    I16,
    I32,
}

struct LevelArray {
    path: String,
    cache: Cache,
    layout: Layout,
    kind: Kind,
}

pub struct Label {
    origin: String,
    name: String,
    frames: u32,
    levels: Vec<LevelArray>,
    geometry: Vec<Level>,
}

impl Label {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn frames(&self) -> u32 {
        self.frames
    }

    pub fn level_count(&self) -> usize {
        self.levels.len()
    }

    pub fn level(&self, index: usize) -> Option<Level> {
        self.geometry.get(index).copied()
    }

    pub fn read(
        &self,
        time: u32,
        level: usize,
        ranges: &[Range<u32>; 3],
        shift: [i64; 3],
    ) -> Result<Vec<u32>, Error> {
        let Some(level) = self.levels.get(level) else {
            return Err(Error::Region {
                array: self.origin.clone(),
                detail: "the requested resolution level does not exist".to_string(),
            });
        };
        let frame = if self.frames == 1 { 0 } else { time };
        if frame >= self.frames {
            return Err(Error::Region {
                array: level.path.clone(),
                detail: format!(
                    "the label image holds {} frames but frame {time} was requested",
                    self.frames
                ),
            });
        }
        let extent = level.layout.extent;
        let limits = [extent.x(), extent.y(), extent.z()];
        for axis in 0..3 {
            if ranges[axis].start >= ranges[axis].end || ranges[axis].end > limits[axis] {
                return Err(Error::Region {
                    array: level.path.clone(),
                    detail: "the region lies outside the label extent".to_string(),
                });
            }
        }
        let dims = [
            ranges[0].end - ranges[0].start,
            ranges[1].end - ranges[1].start,
            ranges[2].end - ranges[2].start,
        ];
        let volume = dims[0] as usize * dims[1] as usize * dims[2] as usize;
        let mut output = vec![0u32; volume];
        let mut source = [0..0, 0..0, 0..0];
        for axis in 0..3 {
            let low = i64::from(ranges[axis].start) + shift[axis];
            let high = i64::from(ranges[axis].end) + shift[axis];
            source[axis] = low.clamp(0, i64::from(limits[axis])) as u32
                ..high.clamp(0, i64::from(limits[axis])) as u32;
        }
        if source.iter().any(Range::is_empty) {
            return Ok(output);
        }
        let subset = ArraySubset::new_with_ranges(&level.layout.ranges(
            u64::from(frame),
            0,
            source[0].clone(),
            source[1].clone(),
            source[2].clone(),
        ));
        let data = retrieve(&level.cache, &subset, &level.path, level.kind)?;
        let widths = [
            (source[0].end - source[0].start) as usize,
            (source[1].end - source[1].start) as usize,
            (source[2].end - source[2].start) as usize,
        ];
        if data.len() != widths[0] * widths[1] * widths[2] {
            return Err(Error::Read {
                array: level.path.clone(),
                detail: "the decoded label region has an unexpected voxel count".to_string(),
            });
        }
        let mut offset = [0i64; 3];
        for axis in 0..3 {
            offset[axis] =
                i64::from(source[axis].start) - (i64::from(ranges[axis].start) + shift[axis]);
        }
        for z in 0..widths[2] {
            for y in 0..widths[1] {
                let row = widths[0] * (y + widths[1] * z);
                for x in 0..widths[0] {
                    let at = [
                        x as i64 + offset[0],
                        y as i64 + offset[1],
                        z as i64 + offset[2],
                    ];
                    if at.iter().any(|value| *value < 0) {
                        continue;
                    }
                    let at = [at[0] as usize, at[1] as usize, at[2] as usize];
                    if at[0] >= dims[0] as usize
                        || at[1] >= dims[1] as usize
                        || at[2] >= dims[2] as usize
                    {
                        continue;
                    }
                    let slot = at[0] + dims[0] as usize * (at[1] + dims[1] as usize * at[2]);
                    output[slot] = data[row + x];
                }
            }
        }
        Ok(output)
    }
}

pub fn read(path: &Path) -> Result<Labels, Error> {
    read_at(&Root::local(path)?, "/")
}

pub fn read_at(root: &Root, group: &str) -> Result<Labels, Error> {
    let labels_group = crate::hcs::join(group, "labels");
    let readable = root.readable();
    if !present(&readable, &labels_group)? {
        return Ok(Labels::empty());
    }
    let opened = Group::open(root.listable(), &labels_group).map_err(|error| Error::Open {
        path: root.tag(&labels_group),
        detail: error.to_string(),
    })?;
    let names = names(&root.tag(&labels_group), opened.attributes())?;
    let cache_bytes = (CACHE_BYTES / names.len().max(1) as u64).max(CACHE_MINIMUM);
    let mut images = Vec::with_capacity(names.len());
    for name in names {
        images.push(image(
            root,
            &labels_group,
            &name,
            readable.clone(),
            cache_bytes,
        )?);
    }
    Ok(Labels { images })
}

fn present(store: &Arc<dyn ReadableStorageTraits>, group: &str) -> Result<bool, Error> {
    for marker in MARKERS {
        let path = crate::hcs::join(group, marker);
        let key = StoreKey::new(path.trim_start_matches('/')).map_err(|error| Error::Open {
            path: group.to_string(),
            detail: error.to_string(),
        })?;
        let value = store.get(&key).map_err(|error| Error::Read {
            array: path.clone(),
            detail: error.to_string(),
        })?;
        if value.is_some() {
            return Ok(true);
        }
    }
    Ok(false)
}

fn names(origin: &str, attributes: &Map<String, Value>) -> Result<Vec<String>, Error> {
    let fields = attributes
        .get("ome")
        .and_then(Value::as_object)
        .unwrap_or(attributes);
    let names = fields
        .get("labels")
        .and_then(Value::as_array)
        .map(|names| {
            names
                .iter()
                .map(|name| {
                    name.as_str()
                        .map(str::to_owned)
                        .ok_or_else(|| Error::Metadata {
                            path: origin.to_string(),
                            detail: "a label name is not a string".to_string(),
                        })
                })
                .collect::<Result<Vec<_>, Error>>()
        })
        .transpose()?
        .unwrap_or_default();
    for name in &names {
        if name.is_empty()
            || name.starts_with('/')
            || name.ends_with('/')
            || name
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
        {
            return Err(Error::Metadata {
                path: origin.to_string(),
                detail: format!("the label image `{name}` is not a relative group path"),
            });
        }
    }
    Ok(names)
}

fn image(
    root: &Root,
    labels_group: &str,
    name: &str,
    store: Arc<dyn ReadableStorageTraits>,
    cache_bytes: u64,
) -> Result<Label, Error> {
    let group_path = crate::hcs::join(labels_group, name);
    let origin = root.tag(&group_path);
    let group = Group::open(root.listable(), &group_path).map_err(|error| Error::Open {
        path: origin.clone(),
        detail: error.to_string(),
    })?;
    let metadata =
        metadata::parse(&origin, group.attributes()).map_err(|error| Error::Metadata {
            path: origin.clone(),
            detail: error.to_string(),
        })?;
    let mut frames = None;
    let mut levels = Vec::with_capacity(metadata.datasets.len());
    for dataset in &metadata.datasets {
        let opened = crate::hcs::join(&group_path, &dataset.path);
        let array = Array::open(store.clone(), &opened).map_err(|error| Error::Open {
            path: origin.clone(),
            detail: error.to_string(),
        })?;
        let kind = kind(&origin, &array)?;
        let layout = axes::layout(
            &metadata.axes,
            array.shape(),
            &dataset.scale,
            &dataset.translation,
        )
        .map_err(|detail| Error::Metadata {
            path: origin.clone(),
            detail,
        })?;
        if layout.channel_count() != 1 {
            return Err(Error::Unsupported {
                path: origin.clone(),
                detail: "a label image holds a single channel".to_string(),
            });
        }
        match frames {
            None => frames = Some(layout.frame_count()),
            Some(known) if known != layout.frame_count() => {
                return Err(Error::Unsupported {
                    path: origin.clone(),
                    detail: "the resolution levels disagree on the frame count".to_string(),
                });
            }
            Some(_) => {}
        }
        levels.push(LevelArray {
            path: opened,
            cache: Cache::new(Arc::new(array), cache_bytes),
            layout,
            kind,
        });
    }
    if levels.is_empty() {
        return Err(Error::Metadata {
            path: origin,
            detail: "the label image has no resolution level".to_string(),
        });
    }
    if !ordered(&levels) {
        return Err(Error::Unsupported {
            path: origin,
            detail: "the label levels are not ordered from fine to coarse".to_string(),
        });
    }
    let (geometry, _) = crate::frame::levels(
        &levels
            .iter()
            .map(|level| level.layout.clone())
            .collect::<Vec<_>>(),
    );
    Ok(Label {
        origin,
        name: name.to_string(),
        frames: frames.expect("a label image has a level"),
        levels,
        geometry,
    })
}

fn ordered(levels: &[LevelArray]) -> bool {
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

fn kind(origin: &str, array: &Array<dyn ReadableStorageTraits>) -> Result<Kind, Error> {
    let data_type: DataType = array.data_type().clone();
    if data_type == uint8() {
        Ok(Kind::U8)
    } else if data_type == uint16() {
        Ok(Kind::U16)
    } else if data_type == uint32() {
        Ok(Kind::U32)
    } else if data_type == uint64() {
        Ok(Kind::U64)
    } else if data_type == int8() {
        Ok(Kind::I8)
    } else if data_type == int16() {
        Ok(Kind::I16)
    } else if data_type == int32() {
        Ok(Kind::I32)
    } else {
        Err(Error::Unsupported {
            path: origin.to_string(),
            detail: format!("the label data type `{data_type}` is not an integer"),
        })
    }
}

fn retrieve(
    cache: &Cache,
    subset: &ArraySubset,
    path: &str,
    kind: Kind,
) -> Result<Vec<u32>, Error> {
    let mut read = |error: zarrs::array::ArrayError| Error::Read {
        array: path.to_string(),
        detail: error.to_string(),
    };
    let options = CodecOptions::default();
    match kind {
        Kind::U8 => Ok(cache
            .retrieve_array_subset::<Vec<u8>>(subset, &options)
            .map_err(&mut read)?
            .into_iter()
            .map(u32::from)
            .collect()),
        Kind::U16 => Ok(cache
            .retrieve_array_subset::<Vec<u16>>(subset, &options)
            .map_err(&mut read)?
            .into_iter()
            .map(u32::from)
            .collect()),
        Kind::U32 => Ok(cache
            .retrieve_array_subset::<Vec<u32>>(subset, &options)
            .map_err(&mut read)?),
        Kind::U64 => cache
            .retrieve_array_subset::<Vec<u64>>(subset, &options)
            .map_err(&mut read)?
            .into_iter()
            .map(|value| {
                u32::try_from(value).map_err(|_| Error::Read {
                    array: path.to_string(),
                    detail: format!("the label {value} exceeds the addressable range"),
                })
            })
            .collect(),
        Kind::I8 => Ok(cache
            .retrieve_array_subset::<Vec<i8>>(subset, &options)
            .map_err(&mut read)?
            .into_iter()
            .map(|value| u32::from(value.max(0) as u8))
            .collect()),
        Kind::I16 => Ok(cache
            .retrieve_array_subset::<Vec<i16>>(subset, &options)
            .map_err(&mut read)?
            .into_iter()
            .map(|value| u32::from(value.max(0) as u16))
            .collect()),
        Kind::I32 => Ok(cache
            .retrieve_array_subset::<Vec<i32>>(subset, &options)
            .map_err(&mut read)?
            .into_iter()
            .map(|value| value.max(0) as u32)
            .collect()),
    }
}
