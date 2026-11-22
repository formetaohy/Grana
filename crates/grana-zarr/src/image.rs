use std::ops::Range;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use grana_volume::{
    Calibration, Census, ChannelInfo, Intensity, Pyramid, Statistics, VoxelData, VoxelFormat,
    VoxelSemantics, name_of, tint_of,
};
use serde_json::{Map, Value};
use zarrs::array::data_type::{
    float32, float64, int8, int16, int32, int64, uint8, uint16, uint32, uint64,
};
use zarrs::array::{Array, ArraySubset, CodecOptions};
use zarrs::group::Group;
use zarrs::storage::ReadableStorageTraits;

use crate::Error;
use crate::axes::{self, Layout};
use crate::cache::{self, Cache};
use crate::metadata::{self, ChannelMetadata};
use crate::outline::Outline;
use crate::root::{Root, Witness};
use crate::scene::Scene;

struct LevelArray {
    path: String,
    cache: Cache,
    layout: Layout,
    witness: OnceLock<Result<Witness, String>>,
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
            let image = GroupImage::open(&root, group, attributes, Footprint::Display, None)?;
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
        Self::open(&root, group, attributes, Footprint::Display, None)
    }

    pub(crate) fn open_member(root: &Root, group: &str, cache_bytes: u64) -> Result<Self, Error> {
        let attributes = crate::hcs::attributes(root, group)?;
        Self::open(
            root,
            group,
            attributes,
            Footprint::Source,
            Some(cache_bytes),
        )
    }

    pub(crate) fn open(
        root: &Root,
        group: &str,
        attributes: Map<String, Value>,
        footprint: Footprint,
        cache_bytes: Option<u64>,
    ) -> Result<Self, Error> {
        let tag = root.tag(group);
        let metadata = metadata::parse(&tag, &attributes)?;
        let datasets = match footprint {
            Footprint::Display => &metadata.datasets,
            Footprint::Source => &metadata.source.datasets,
        };
        let readable: Arc<dyn ReadableStorageTraits> = root.readable();
        let cache_bytes = cache_bytes.unwrap_or_else(|| cache::per_level(datasets.len()));
        let mut levels = Vec::with_capacity(datasets.len());
        let mut channel_count = None;
        let mut frames = None;
        let mut interval = None;
        let mut format = None;
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
                cache: Cache::open(array, cache_bytes),
                layout,
                witness: OnceLock::new(),
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
        let semantics = if metadata.label_image {
            VoxelSemantics::Label
        } else {
            VoxelSemantics::Scalar
        };
        let channels = channels(
            &root.tag(group),
            &metadata.channels,
            channel_count,
            format,
            &levels,
            frames,
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
        })
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
        let levels = self
            .levels
            .iter()
            .map(|level| {
                let metadata = serde_json::to_vec(level.cache.array().metadata()).ok()?;
                Some((level.path.clone(), metadata))
            })
            .collect::<Option<Vec<_>>>()?;
        crate::identity::dataset(self.root.path(), &levels)
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
        let ranges = level
            .layout
            .ranges(u64::from(time), channel as u64, x, y, z);
        let subset = ArraySubset::new_with_ranges(&ranges);
        let array = level.cache.array();
        let Some(chunks) = array
            .chunks_in_array_subset(&subset)
            .map_err(|error| Error::Read {
                array: level.path.clone(),
                detail: error.to_string(),
            })?
        else {
            return Ok(None);
        };
        let keys = chunk_keys(&array, chunks.start(), chunks.shape());
        let witness = level
            .witness
            .get_or_init(|| {
                self.root
                    .witness(&level.path)
                    .map_err(|error| error.to_string())
            })
            .as_ref()
            .map_err(|detail| Error::Read {
                array: level.path.clone(),
                detail: detail.clone(),
            })?;
        witness.stamp(keys.into_iter())
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

fn retrieve(cache: &Cache, subset: &ArraySubset, path: &str) -> Result<VoxelData, Error> {
    let mut read = |error: zarrs::array::ArrayError| Error::Read {
        array: path.to_string(),
        detail: error.to_string(),
    };
    let options = CodecOptions::default();
    let array = cache.array();
    let data_type = array.data_type();
    macro_rules! held {
        ($variant:ident, $type:ty) => {
            Ok(VoxelData::$variant(
                cache
                    .retrieve::<$type>(subset, &options)
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
    origin: &str,
    metadata: &[ChannelMetadata],
    count: u32,
    format: VoxelFormat,
    levels: &[LevelArray],
    frames: u32,
    semantics: VoxelSemantics,
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
                .unwrap_or_else(|| name_of(index));
            let tint = metadata
                .map(|metadata| metadata.tint)
                .unwrap_or_else(|| tint_of(index));
            let data_range = metadata.and_then(|metadata| metadata.data).and_then(valid);
            let omero_window = metadata
                .and_then(|metadata| metadata.window)
                .and_then(valid);
            let statistics = match data_range {
                Some(data) => Statistics::declared(data, semantics),
                None => {
                    let sampled = sample_range(levels, index as usize, frames)?;
                    let sampled = sampled.ok_or_else(|| Error::Metadata {
                        path: origin.to_string(),
                        detail: format!("the channel {index} exposes no voxel to census"),
                    })?;
                    Statistics::declared(sampled.intensity(), semantics)
                }
            };
            let info = ChannelInfo::new(name, tint, statistics, format);
            Ok(match omero_window {
                Some(window) => info.with_window(window),
                None => info,
            })
        })
        .collect()
}

fn sample_range(
    levels: &[LevelArray],
    channel: usize,
    frames: u32,
) -> Result<Option<Statistics>, Error> {
    let level = levels.last().expect("a multiscale image has a level");
    let mut census = Census::of(level.layout.extent, frames);
    let measured = census.measure(|time, ranges| {
        let ranges = level.layout.ranges(
            u64::from(time),
            channel as u64,
            ranges[0].clone(),
            ranges[1].clone(),
            ranges[2].clone(),
        );
        let subset = ArraySubset::new_with_ranges(&ranges);
        retrieve(&level.cache, &subset, &level.path)
    })?;
    Ok(Some(measured))
}

fn valid(range: [f32; 2]) -> Option<Intensity> {
    (range[0].is_finite() && range[1].is_finite() && range[0] < range[1])
        .then(|| Intensity::new(range[0], range[1]))
}
