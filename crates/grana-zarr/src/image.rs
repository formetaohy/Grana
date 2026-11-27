use std::ops::Range;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use grana_volume::{
    Calibration, Census, ChannelInfo, Intensity, Measured, NoData, Pyramid, Statistics, VoxelData,
    VoxelFormat, VoxelSemantics, name_of, tint_of,
};
use serde_json::{Map, Value};
use zarrs::array::chunk_grid::api::ChunkShapeTraits;
use zarrs::array::data_type::{
    float32, float64, int8, int16, int32, int64, uint8, uint16, uint32, uint64,
};
use zarrs::array::{Array, ArrayShardedExt, ArraySubset};
use zarrs::group::Group;
use zarrs::storage::ReadableStorageTraits;

use crate::Error;
use crate::axes::{self, Layout};
use crate::metadata::{self, ChannelMetadata};
use crate::outline::Outline;
use crate::root::Root;
use crate::scene::Scene;
use crate::tile::{Decode, Tiles};

struct LevelArray {
    path: String,
    array: Arc<Array<dyn ReadableStorageTraits>>,
    layout: Layout,
    unit: [u32; 3],
    no_data: NoData,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Footprint {
    Display,
    Source,
}

pub struct Image {
    kind: Kind,
}

enum Kind {
    Group(GroupImage),
    Scene(Scene),
}

pub struct GroupImage {
    root: Root,
    group: String,
    name: String,
    version: String,
    minor: u32,
    levels: Vec<LevelArray>,
    pyramid: Pyramid,
    axes: Vec<metadata::Axis>,
    datasets: Vec<metadata::Dataset>,
    systems: Vec<metadata::System>,
    intrinsic: Option<String>,
    tiles: Tiles,
    identity: Option<u64>,
}

impl Image {
    pub fn open(path: &Path) -> Result<Self, Error> {
        Self::open_root(Root::local(path)?, "/")
    }

    pub fn open_group(path: &Path, group: &str) -> Result<Self, Error> {
        Self::open_root(Root::local(path)?, group)
    }

    pub fn outline(root: &Root, group: &str) -> Result<Outline, Error> {
        GroupImage::outline(root, group)
    }

    pub fn open_root(root: Root, group: &str) -> Result<Self, Error> {
        let attributes = crate::hcs::attributes(&root, group)?;
        let fields = crate::hcs::fields(&attributes);
        if fields.contains_key("multiscales") {
            let image = GroupImage::open(&root, group, attributes, Footprint::Display)?;
            return Ok(Self {
                kind: Kind::Group(image),
            });
        }
        if fields.contains_key("scene") {
            let scene = Scene::open(root, group, &attributes)?;
            return Ok(Self {
                kind: Kind::Scene(scene),
            });
        }
        Err(metadata::absent(&root.tag(group), &attributes, fields))
    }

    pub fn decode(&self) -> Decode {
        match &self.kind {
            Kind::Group(image) => image.decode(),
            Kind::Scene(scene) => scene.decode(),
        }
    }

    pub fn empty(
        &self,
        time: u32,
        level: usize,
        coord: grana_volume::BrickCoord,
    ) -> Result<Option<NoData>, Error> {
        match &self.kind {
            Kind::Group(image) => image.empty(time, level, coord),
            Kind::Scene(_) => Ok(None),
        }
    }

    pub fn name(&self) -> &str {
        match &self.kind {
            Kind::Group(image) => image.name(),
            Kind::Scene(scene) => scene.name(),
        }
    }

    pub fn transfer(&self) -> Option<Arc<grana_net::Transfer>> {
        match &self.kind {
            Kind::Group(image) => image.transfer(),
            Kind::Scene(scene) => scene.transfer(),
        }
    }

    pub fn identity(&self) -> Option<u64> {
        match &self.kind {
            Kind::Group(image) => image.identity(),
            Kind::Scene(scene) => scene.identity(),
        }
    }

    pub fn stamps(
        &self,
        level: usize,
        time: u32,
        channel: usize,
        x: Range<u32>,
        y: Range<u32>,
        z: Range<u32>,
    ) -> Result<Option<u64>, Error> {
        match &self.kind {
            Kind::Group(image) => image.stamps(level, time, channel, x, y, z),
            Kind::Scene(scene) => scene.stamps(level, time, channel, x, y, z),
        }
    }

    pub fn labels(&self) -> Result<crate::Labels, Error> {
        match &self.kind {
            Kind::Group(image) => image.labels(),
            Kind::Scene(_) => Ok(crate::Labels::empty()),
        }
    }

    pub fn pyramid(&self) -> &Pyramid {
        match &self.kind {
            Kind::Group(image) => image.pyramid(),
            Kind::Scene(scene) => scene.pyramid(),
        }
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
        match &self.kind {
            Kind::Group(image) => image.read_region(time, level, channel, x, y, z),
            Kind::Scene(scene) => scene.read_region(time, level, channel, x, y, z),
        }
    }
}

impl GroupImage {
    pub fn open_group(path: &Path, group: &str) -> Result<Self, Error> {
        let root = Root::local(path)?;
        let attributes = crate::hcs::attributes(&root, group)?;
        Self::open(&root, group, attributes, Footprint::Display)
    }

    pub(crate) fn open_member(root: &Root, group: &str) -> Result<Self, Error> {
        let attributes = crate::hcs::attributes(root, group)?;
        Self::open(root, group, attributes, Footprint::Source)
    }

    pub(crate) fn open(
        root: &Root,
        group: &str,
        attributes: Map<String, Value>,
        footprint: Footprint,
    ) -> Result<Self, Error> {
        let tag = root.tag(group);
        let metadata = metadata::parse(&tag, &attributes)?;
        let datasets = match footprint {
            Footprint::Display => &metadata.datasets,
            Footprint::Source => &metadata.source.datasets,
        };
        let readable: Arc<dyn ReadableStorageTraits> = root.readable();
        let mut levels = Vec::with_capacity(datasets.len());
        let mut channel_count = None;
        let mut frames = None;
        let mut interval = None;
        let mut format = None;
        let mut no_data = None;
        for dataset in datasets {
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
            let level_no_data = declared_no_data(&array_path, &array, level_format)?;
            match no_data {
                None => no_data = Some(level_no_data),
                Some(known) if known != level_no_data => {
                    return Err(Error::Unsupported {
                        path: tag.clone(),
                        detail: "the resolution levels disagree on the fill value".to_string(),
                    });
                }
                Some(_) => {}
            }
            let layout =
                axes::layout(&metadata.axes, array.shape(), dataset).map_err(|detail| {
                    Error::Unsupported {
                        path: tag.clone(),
                        detail,
                    }
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
                unit: unit_shape(&array, &layout),
                array,
                layout,
                no_data: level_no_data,
            });
        }
        assert!(
            !levels.is_empty(),
            "a multiscale image has at least one dataset"
        );
        let frames = frames.expect("a multiscale image has at least one dataset");
        let interval = interval.expect("a multiscale image has at least one dataset");
        if !levels_ordered(&levels) {
            return Err(Error::Unsupported {
                path: tag,
                detail: "the multiscale levels are not ordered from fine to coarse".to_string(),
            });
        }
        let format = format.expect("a multiscale image has at least one dataset");
        let no_data = no_data.expect("a multiscale image has at least one dataset");
        let semantics = if metadata.label_image {
            VoxelSemantics::Label
        } else {
            VoxelSemantics::Scalar
        };
        let origin = root.tag(group);
        let identity = dataset_digest(root.path(), &levels);
        let tiles = Tiles::new();
        let channels = channels(
            &tiles,
            &origin,
            &metadata.channels,
            format,
            no_data,
            &levels,
            semantics,
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
        )
        .map_err(|detail| Error::Unsupported {
            path: root.tag(group),
            detail,
        })?;
        let stored = level_metadata.len();
        let calibration = metadata.calibration.unwrap_or_else(|| {
            if datasets.iter().all(|dataset| dataset.placed) {
                Calibration::Recorded
            } else {
                Calibration::Uncalibrated
            }
        });
        Ok(Self {
            root: root.clone(),
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
                calibration,
            ),
            axes: metadata.axes,
            datasets: datasets.clone(),
            systems: metadata.systems,
            intrinsic: metadata.source.name,
            tiles,
            identity,
        })
    }

    pub(crate) fn decode(&self) -> Decode {
        self.tiles.stats()
    }

    fn outline(root: &Root, group: &str) -> Result<Outline, Error> {
        let tag = root.tag(group);
        let opened = Group::open(root.listable(), group).map_err(|error| Error::Open {
            path: tag.clone(),
            detail: error.to_string(),
        })?;
        let metadata = metadata::parse(&tag, opened.attributes())?;
        let dataset = metadata
            .datasets
            .first()
            .expect("a multiscale image lists a dataset");
        let array_path = crate::hcs::join(group, &dataset.path);
        let array = open_array(root.readable(), &array_path)?;
        let layout = axes::layout(&metadata.axes, array.shape(), dataset)
            .map_err(|detail| Error::Unsupported { path: tag, detail })?;
        let channels = u32::try_from(layout.channel_count()).map_err(|_| Error::Unsupported {
            path: array_path,
            detail: "the channel axis is too long".to_string(),
        })?;
        Ok(Outline::new(layout.extent, channels, layout.frame_count()))
    }

    pub(crate) fn version(&self) -> &str {
        &self.version
    }

    pub(crate) fn minor(&self) -> u32 {
        self.minor
    }

    pub(crate) fn axes(&self) -> &[metadata::Axis] {
        &self.axes
    }

    pub(crate) fn datasets(&self) -> &[metadata::Dataset] {
        &self.datasets
    }

    pub(crate) fn systems(&self) -> &[metadata::System] {
        &self.systems
    }

    pub(crate) fn intrinsic(&self) -> Option<&str> {
        self.intrinsic.as_deref()
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn transfer(&self) -> Option<Arc<grana_net::Transfer>> {
        self.root.transfer()
    }

    pub fn identity(&self) -> Option<u64> {
        self.identity
    }

    pub fn stamps(
        &self,
        level: usize,
        time: u32,
        channel: usize,
        x: Range<u32>,
        y: Range<u32>,
        z: Range<u32>,
    ) -> Result<Option<u64>, Error> {
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
        let ranges = level
            .layout
            .ranges(u64::from(time), channel as u64, x, y, z);
        let subset = ArraySubset::new_with_ranges(&ranges);
        let array = &level.array;
        let Some(chunks) = array
            .chunks_in_array_subset(&subset)
            .map_err(|error| Error::Read {
                array: level.path.clone(),
                detail: error.to_string(),
            })?
        else {
            return Ok(None);
        };
        let keys = chunk_keys(array, chunks.start(), chunks.shape());
        self.root.witness(&level.path).stamp(keys.into_iter())
    }

    pub fn labels(&self) -> Result<crate::Labels, Error> {
        let layouts = self
            .levels
            .iter()
            .map(|level| level.layout.clone())
            .collect::<Vec<_>>();
        crate::label::read_at(&self.root, &self.group, &layouts)
    }

    pub fn pyramid(&self) -> &Pyramid {
        &self.pyramid
    }

    fn empty(
        &self,
        time: u32,
        level: usize,
        coord: grana_volume::BrickCoord,
    ) -> Result<Option<NoData>, Error> {
        if self.root.path().is_none() {
            return Ok(None);
        }
        let Some(held) = self.levels.get(level) else {
            return Ok(None);
        };
        if time >= held.layout.frame_count() {
            return Ok(None);
        }
        let grid = grana_volume::BrickGrid::new(held.layout.extent);
        let counts = grid.counts();
        if coord.x() >= counts[0] || coord.y() >= counts[1] || coord.z() >= counts[2] {
            return Ok(None);
        }
        let ranges = grid.padded_range(coord);
        let storage = held.array.storage();
        for channel in 0..held.layout.channel_count() {
            let subset = ArraySubset::new_with_ranges(&held.layout.ranges(
                u64::from(time),
                channel,
                ranges[0].clone(),
                ranges[1].clone(),
                ranges[2].clone(),
            ));
            let Some(chunks) = held
                .array
                .chunks_in_array_subset(&subset)
                .map_err(|error| Error::Read {
                    array: held.path.clone(),
                    detail: error.to_string(),
                })?
            else {
                return Ok(None);
            };
            for indices in chunks.indices() {
                let present =
                    storage
                        .size_key(&held.array.chunk_key(&indices))
                        .map_err(|error| Error::Read {
                            array: held.path.clone(),
                            detail: error.to_string(),
                        })?;
                if present.is_some() {
                    return Ok(None);
                }
            }
        }
        Ok(Some(held.no_data))
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
        let index = level;
        let Some(level) = self.levels.get(index) else {
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
        let ranges = [x.clone(), y.clone(), z.clone()];
        let unit = crate::tile::Unit {
            level: index,
            array: &level.array,
            layout: &level.layout,
            path: &level.path,
            shape: level.unit,
            no_data: level.no_data,
        };
        let data = self.tiles.read(&unit, time, channel as u32, &ranges)?;
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

pub fn outlines(root: &Root, groups: &[&str]) -> Result<Vec<Outline>, Error> {
    if groups.is_empty() {
        return Ok(Vec::new());
    }
    let workers = crate::parallel::threads().min(groups.len());
    let next = AtomicUsize::new(0);
    let stopped = AtomicBool::new(false);
    let resolved = Mutex::new(vec![None; groups.len()]);
    let failure = Mutex::new(None);
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                while !stopped.load(Ordering::Relaxed) {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    if index >= groups.len() {
                        break;
                    }
                    match Image::outline(root, groups[index]) {
                        Ok(outline) => {
                            resolved.lock().unwrap_or_else(|error| error.into_inner())[index] =
                                Some(outline);
                        }
                        Err(error) => {
                            let mut held =
                                failure.lock().unwrap_or_else(|error| error.into_inner());
                            if held.is_none() {
                                *held = Some(error);
                            }
                            stopped.store(true, Ordering::Relaxed);
                            break;
                        }
                    }
                }
            });
        }
    });
    if let Some(error) = failure
        .into_inner()
        .unwrap_or_else(|error| error.into_inner())
    {
        return Err(error);
    }
    Ok(resolved
        .into_inner()
        .unwrap_or_else(|error| error.into_inner())
        .into_iter()
        .map(|outline| outline.expect("every asked outline resolves in its group order"))
        .collect())
}

fn chunk_keys(
    array: &Array<dyn ReadableStorageTraits>,
    start: &[u64],
    shape: &[u64],
) -> Vec<String> {
    if shape.contains(&0) {
        return Vec::new();
    }
    let mut keys = Vec::new();
    let mut cursor = vec![0u64; start.len()];
    loop {
        let indices = start
            .iter()
            .zip(&cursor)
            .map(|(start, step)| start + step)
            .collect::<Vec<_>>();
        keys.push(array.chunk_key(&indices).as_str().to_string());
        let mut carried = true;
        for axis in (0..cursor.len()).rev() {
            cursor[axis] += 1;
            if cursor[axis] < shape[axis] {
                carried = false;
                break;
            }
            cursor[axis] = 0;
        }
        if carried {
            return keys;
        }
    }
}

fn levels_ordered(levels: &[LevelArray]) -> bool {
    levels.windows(2).all(|pair| {
        let fine = &pair[0].layout;
        let coarse = &pair[1].layout;
        let (fine_extent, coarse_extent) = (fine.extent, coarse.extent);
        let (fine_spacing, coarse_spacing) = (fine.voxel(), coarse.voxel());
        fine_extent.x() >= coarse_extent.x()
            && fine_extent.y() >= coarse_extent.y()
            && fine_extent.z() >= coarse_extent.z()
            && fine_spacing[0] <= coarse_spacing[0]
            && fine_spacing[1] <= coarse_spacing[1]
            && fine_spacing[2] <= coarse_spacing[2]
    })
}

fn dataset_digest(directory: Option<&Path>, levels: &[LevelArray]) -> Option<u64> {
    let metadata = levels
        .iter()
        .map(|level| {
            let array = serde_json::to_vec(level.array.metadata())
                .expect("the array metadata of a level serializes to json");
            (level.path.clone(), array)
        })
        .collect::<Vec<_>>();
    crate::identity::dataset(directory, &metadata)
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

pub(crate) fn supported_format(
    path: &str,
    array: &Array<dyn ReadableStorageTraits>,
) -> Result<VoxelFormat, Error> {
    let data_type = array.data_type();
    let formats = [
        (uint8(), VoxelFormat::U8),
        (uint16(), VoxelFormat::U16),
        (uint32(), VoxelFormat::U32),
        (uint64(), VoxelFormat::U64),
        (int8(), VoxelFormat::I8),
        (int16(), VoxelFormat::I16),
        (int32(), VoxelFormat::I32),
        (int64(), VoxelFormat::I64),
        (float32(), VoxelFormat::F32),
        (float64(), VoxelFormat::F64),
    ];
    formats
        .into_iter()
        .find_map(|(held, format)| (data_type == &held).then_some(format))
        .ok_or_else(|| Error::Unsupported {
            path: path.to_string(),
            detail: format!("the voxel data type `{data_type}` is not supported"),
        })
}

fn unit_shape(array: &Array<dyn ReadableStorageTraits>, layout: &Layout) -> [u32; 3] {
    let shape = if array.is_sharded() {
        array
            .effective_subchunk_shape()
            .map(|shape| shape.to_array_shape())
    } else {
        let indices = vec![0u64; array.chunk_grid_shape().len()];
        array
            .chunk_shape(&indices)
            .ok()
            .map(|shape| shape.to_array_shape())
    };
    layout.spatial(shape.as_deref().unwrap_or(array.shape()))
}

pub(crate) fn retrieve(
    array: &Array<dyn ReadableStorageTraits>,
    subset: &ArraySubset,
    path: &str,
) -> Result<VoxelData, Error> {
    let mut read = |error: zarrs::array::ArrayError| Error::Read {
        array: path.to_string(),
        detail: error.to_string(),
    };
    let data_type = array.data_type();
    macro_rules! held {
        ($variant:ident, $type:ty) => {
            Ok(VoxelData::$variant(
                array
                    .retrieve_array_subset::<Vec<$type>>(subset)
                    .map_err(&mut read)?,
            ))
        };
    }
    if data_type == &uint8() {
        held!(U8, u8)
    } else if data_type == &uint16() {
        held!(U16, u16)
    } else if data_type == &uint32() {
        held!(U32, u32)
    } else if data_type == &uint64() {
        held!(U64, u64)
    } else if data_type == &int8() {
        held!(I8, i8)
    } else if data_type == &int16() {
        held!(I16, i16)
    } else if data_type == &int32() {
        held!(I32, i32)
    } else if data_type == &int64() {
        held!(I64, i64)
    } else if data_type == &float64() {
        held!(F64, f64)
    } else if data_type == &float32() {
        held!(F32, f32)
    } else {
        Err(Error::Unsupported {
            path: path.to_string(),
            detail: format!("the voxel data type `{data_type}` is not supported"),
        })
    }
}

fn channels(
    tiles: &Tiles,
    origin: &str,
    metadata: &[ChannelMetadata],
    format: VoxelFormat,
    no_data: NoData,
    levels: &[LevelArray],
    semantics: VoxelSemantics,
) -> Result<Vec<ChannelInfo>, Error> {
    let count = levels
        .first()
        .map(|level| level.layout.channel_count())
        .and_then(|count| u32::try_from(count).ok())
        .expect("a multiscale image holds one to 4G channels");
    let frames = levels
        .first()
        .map(|level| level.layout.frame_count())
        .expect("a multiscale image holds at least one level");
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
                .unwrap_or_else(|| name_of(index));
            let tint = metadata
                .map(|metadata| metadata.tint)
                .unwrap_or_else(|| tint_of(index));
            if let Some(color_table) = metadata.and_then(|metadata| metadata.color_table.clone()) {
                return Ok(
                    ChannelInfo::declared(name, tint, color_table.indices(), format)
                        .with_color_table(color_table)
                        .with_no_data(no_data),
                );
            }
            let data_range = metadata.and_then(|metadata| metadata.data).and_then(valid);
            let omero_window = metadata
                .and_then(|metadata| metadata.window)
                .and_then(valid);
            let sampled = match data_range {
                Some(_) => None,
                None => Some(
                    sample_range(tiles, levels, index as usize, frames, no_data)?.ok_or_else(
                        || Error::Metadata {
                            path: origin.to_string(),
                            detail: format!("the channel {index} exposes no voxel to census"),
                        },
                    )?,
                ),
            };
            let statistics = match &sampled {
                Some(sampled) => Statistics::declared(sampled.statistics().intensity(), semantics),
                None => Statistics::declared(
                    data_range.expect("a declared channel names its own range"),
                    semantics,
                ),
            };
            let info = ChannelInfo::new(name, tint, statistics, format);
            let info = info.with_no_data(no_data);
            let info = match sampled {
                Some(sampled) => info.with_measured(sampled),
                None => info,
            };
            Ok(match omero_window {
                Some(window) => info.with_window(window),
                None => info,
            })
        })
        .collect()
}

fn sample_range(
    tiles: &Tiles,
    levels: &[LevelArray],
    channel: usize,
    frames: u32,
    no_data: NoData,
) -> Result<Option<Measured>, Error> {
    let index = levels.len() - 1;
    let level = levels.last().expect("a multiscale image has a level");
    let unit = crate::tile::Unit {
        level: index,
        array: &level.array,
        layout: &level.layout,
        path: &level.path,
        shape: level.unit,
        no_data: level.no_data,
    };
    let mut census = Census::of(level.layout.extent, frames);
    let measured = census.measure(no_data.declared(), |time, ranges| {
        tiles.read(&unit, time, channel as u32, &ranges)
    })?;
    Ok(Some(measured))
}

fn declared_no_data(
    path: &str,
    array: &Array<dyn ReadableStorageTraits>,
    format: VoxelFormat,
) -> Result<NoData, Error> {
    let fill = array.fill_value();
    let bytes = fill.as_ne_bytes();
    if bytes.len() != format.bytes() {
        return Err(Error::Unsupported {
            path: path.to_string(),
            detail: "the fill value does not hold one sample of the voxel data type".to_string(),
        });
    }
    macro_rules! exact {
        ($width:expr, $type:ty, $variant:ident) => {{
            let at: [u8; $width] = bytes
                .try_into()
                .expect("a checked fill value holds one sample");
            NoData::$variant(<$type>::from_ne_bytes(at))
        }};
    }
    Ok(match format {
        VoxelFormat::U8 => exact!(1, u8, U8),
        VoxelFormat::U16 => exact!(2, u16, U16),
        VoxelFormat::U32 => exact!(4, u32, U32),
        VoxelFormat::U64 => exact!(8, u64, U64),
        VoxelFormat::I8 => exact!(1, i8, I8),
        VoxelFormat::I16 => exact!(2, i16, I16),
        VoxelFormat::I32 => exact!(4, i32, I32),
        VoxelFormat::I64 => exact!(8, i64, I64),
        VoxelFormat::F32 => exact!(4, f32, F32),
        VoxelFormat::F64 => exact!(8, f64, F64),
    })
}

fn valid(range: [f32; 2]) -> Option<Intensity> {
    (range[0].is_finite() && range[1].is_finite() && range[0] < range[1])
        .then(|| Intensity::new(range[0], range[1]))
}
