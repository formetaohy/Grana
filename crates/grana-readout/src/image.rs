use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use grana_volume::{
    Axial, ChannelInfo, Extent, Intensity, Level, Pyramid, Spacing, VoxelData, VoxelFormat,
    name_of, tint_of,
};
use openreadout_core::region::{self, ResolutionLevel};
use openreadout_core::{ImageInfo, PixelType, Plane, PlaneIndex};

use crate::error::Error;
use crate::family::Family;
use crate::open;
use crate::plane::{self, View};
use crate::probe;
use crate::reader::{self, Readers};
use crate::scene::{self, Scene};

const CACHE_BYTES: usize = 256 << 20;

const RED: [f32; 3] = [1.0, 0.0, 0.0];
const GREEN: [f32; 3] = [0.0, 1.0, 0.0];
const BLUE: [f32; 3] = [0.0, 0.0, 1.0];

pub struct Image {
    path: PathBuf,
    family: Family,
    name: String,
    index: u32,
    scenes: Vec<Scene>,
    frames: u32,
    pixel: PixelType,
    samples: u32,
    views: Vec<View>,
    pyramid: Pyramid,
    readers: Readers,
    cache: plane::Shards,
}

impl Image {
    pub fn open(path: &Path, series: usize) -> Result<Self, Error> {
        let document = open::open(path)?;
        let scenes = scene::describe(path, &document.info)?;
        let image = document.info.images.get(series).ok_or_else(|| {
            Error::unsupported(
                path,
                format!(
                    "the file holds {} images but image {series} was requested",
                    document.info.images.len()
                ),
            )
        })?;
        let scene = &scenes[series];
        let samples = image.samples_per_pixel.max(1);
        let format = format_of(image.pixel_type).ok_or_else(|| {
            Error::unsupported(path, "the image stores a sample type grana cannot read")
        })?;
        let views = views(image, format);
        let (unit, axial) = spacing(image);
        let levels = levels(path, image, unit)?;
        let frames = scene.frames();
        let interval = interval(image);
        let mut dataset = document.dataset;
        let mut probe_cache = plane::Cache::new(CACHE_BYTES);
        let probes = probe::intensities(path, &mut *dataset, image, &views, &mut probe_cache)?;
        let channels = views
            .iter()
            .enumerate()
            .map(|(index, view)| channel(image, *view, index, probes[index]))
            .collect();
        let stored = levels.len();
        let pyramid = Pyramid::new(levels, stored, channels, frames, interval, axial);
        Ok(Self {
            path: path.to_path_buf(),
            family: document.family,
            name: scene
                .name()
                .map(str::to_string)
                .unwrap_or_else(|| scene::dataset_name(path)),
            index: image.index,
            scenes,
            frames,
            pixel: image.pixel_type,
            samples,
            views,
            pyramid,
            readers: Readers::new(path.to_path_buf(), dataset, reader::limit()),
            cache: plane::Shards::new(CACHE_BYTES),
        })
    }

    pub fn family(&self) -> Family {
        self.family
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn scenes(&self) -> &[Scene] {
        &self.scenes
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
        let extent = self.extent(level)?;
        if time >= self.frames {
            return Err(Error::region(
                &self.path,
                "the requested frame lies outside the image",
            ));
        }
        let view = *self.views.get(channel).ok_or_else(|| {
            Error::region(&self.path, "the requested channel lies outside the image")
        })?;
        let ranges = [x, y, z];
        self.clamp(&ranges, extent)?;
        let width = (ranges[0].end - ranges[0].start) as usize;
        let height = (ranges[1].end - ranges[1].start) as usize;
        let depth = (ranges[2].end - ranges[2].start) as usize;
        let mut data = VoxelData::blank(view.format, width * height * depth);
        match &mut data {
            VoxelData::U8(values) => self.read_planes(time, level, view, &ranges, values)?,
            VoxelData::U16(values) => self.read_planes(time, level, view, &ranges, values)?,
            VoxelData::F32(values) => self.read_planes(time, level, view, &ranges, values)?,
            _ => {
                return Err(Error::region(
                    &self.path,
                    "the scene decodes a voxel type this platform cannot hold",
                ));
            }
        }
        Ok(data)
    }

    fn read_planes<T: plane::Sample>(
        &self,
        time: u32,
        level: usize,
        view: View,
        ranges: &[Range<u32>; 3],
        values: &mut [T],
    ) -> Result<(), Error> {
        let x = &ranges[0];
        let y = &ranges[1];
        let z = &ranges[2];
        let slice = (x.end - x.start) as usize * (y.end - y.start) as usize;
        let depth = (z.end - z.start) as usize;
        assert_eq!(
            values.len(),
            slice * depth,
            "a plane block covers exactly the requested region"
        );
        let threads = depth.min(self.readers.limit()).max(1);
        let read = |start: u32, block: &mut [T]| -> Result<(), Error> {
            for (index, target) in block.chunks_mut(slice).enumerate() {
                let plane = self.plane(time, level, view, start + index as u32)?;
                plane::rows(&plane, view.sample, x, y, target);
            }
            Ok(())
        };
        if threads == 1 {
            return read(z.start, values);
        }
        let stride = depth.div_ceil(threads) * slice;
        let outcomes: Vec<Result<(), Error>> = std::thread::scope(|scope| {
            let mut handles = Vec::with_capacity(threads);
            let mut start = z.start;
            for block in values.chunks_mut(stride) {
                let planes = block.len() / slice;
                handles.push(scope.spawn(move || read(start, block)));
                start += planes as u32;
            }
            handles
                .into_iter()
                .map(|handle| match handle.join() {
                    Ok(outcome) => outcome,
                    Err(panic) => std::panic::resume_unwind(panic),
                })
                .collect()
        });
        for outcome in outcomes {
            outcome?;
        }
        Ok(())
    }

    fn extent(&self, level: usize) -> Result<Extent, Error> {
        self.pyramid
            .levels()
            .get(level)
            .map(|level| level.extent())
            .ok_or_else(|| {
                Error::unsupported(
                    &self.path,
                    format!(
                        "the microscope image stores {} resolution levels but level {level} was requested",
                        self.pyramid.level_count()
                    ),
                )
            })
    }

    fn clamp(&self, ranges: &[Range<u32>; 3], extent: Extent) -> Result<(), Error> {
        let [x, y, z] = ranges;
        if x.end <= x.start || y.end <= y.start || z.end <= z.start {
            return Err(Error::region(&self.path, "a requested region is empty"));
        }
        if x.end > extent.x() || y.end > extent.y() || z.end > extent.z() {
            return Err(Error::region(
                &self.path,
                "the requested region leaves the image",
            ));
        }
        Ok(())
    }

    fn plane(&self, time: u32, level: usize, view: View, z: u32) -> Result<Arc<Plane>, Error> {
        let key = (
            u32::try_from(level).expect("a pyramid holds fewer than 4G levels"),
            time,
            view.channel,
            z,
        );
        self.cache.fetch(&key, || self.load_plane(key, view))
    }

    fn load_plane(&self, key: plane::Key, view: View) -> Result<Arc<Plane>, Error> {
        let extent = self.extent(key.0 as usize)?;
        let plane = self.readers.with(|dataset| {
            dataset
                .read_plane_level(
                    self.index,
                    PlaneIndex {
                        c: view.channel,
                        z: key.3,
                        t: key.1,
                    },
                    key.0,
                )
                .map_err(|error| Error::read(&self.path, error.to_string()))
        })?;
        if plane.pixel_type != self.pixel || plane.samples_per_pixel != self.samples {
            return Err(Error::read(
                &self.path,
                "a plane disagrees with the declared sample type",
            ));
        }
        if plane.width != extent.x() || plane.height != extent.y() {
            return Err(Error::read(
                &self.path,
                "a plane disagrees with the declared extent",
            ));
        }
        if plane.data.len() != plane.expected_len() {
            return Err(Error::read(&self.path, "a plane holds an unexpected size"));
        }
        Ok(Arc::new(plane))
    }
}

fn levels(path: &Path, image: &ImageInfo, unit: Spacing) -> Result<Vec<Level>, Error> {
    let base = Extent::new(image.size_x, image.size_y, image.size_z);
    region::levels_of(image)
        .iter()
        .map(|level| {
            let extent = level_extent(path, level, base)?;
            let scale = |axis: u32, finest: u32| finest as f32 / axis as f32;
            Ok(Level::new(
                extent,
                Spacing::new(
                    unit.x() * scale(extent.x(), base.x()),
                    unit.y() * scale(extent.y(), base.y()),
                    unit.z() * scale(extent.z(), base.z()),
                ),
            ))
        })
        .collect()
}

fn level_extent(path: &Path, level: &ResolutionLevel, base: Extent) -> Result<Extent, Error> {
    let size = [level.size_x, level.size_y, level.size_z.unwrap_or(base.z())];
    if size.contains(&0) {
        return Err(Error::unsupported(
            path,
            format!("resolution level {} declares an empty extent", level.level),
        ));
    }
    Ok(Extent::new(size[0], size[1], size[2]))
}

fn views(image: &ImageInfo, format: VoxelFormat) -> Vec<View> {
    let channels = image.size_c.max(1);
    let samples = image.samples_per_pixel.max(1);
    (0..channels)
        .flat_map(|channel| {
            (0..samples).map(move |sample| View {
                channel,
                sample: sample as usize,
                format,
            })
        })
        .collect()
}

fn channel(image: &ImageInfo, view: View, index: usize, probe: Option<Intensity>) -> ChannelInfo {
    let source = image
        .channels
        .iter()
        .find(|channel| channel.index == view.channel);
    let base = source
        .and_then(|channel| channel.name.as_deref().or(channel.fluorophore.as_deref()))
        .filter(|name| !name.trim().is_empty())
        .map_or_else(|| name_of(view.channel), str::to_string);
    let (name, tint) = if image.samples_per_pixel <= 1 {
        let tint = source
            .and_then(|channel| channel.color.as_deref())
            .and_then(color)
            .unwrap_or_else(|| tint_of(index as u32));
        (base, tint)
    } else {
        let (suffix, tint) = match view.sample {
            0 => ("Red", RED),
            1 => ("Green", GREEN),
            _ => ("Blue", BLUE),
        };
        (format!("{base} {suffix}"), tint)
    };
    ChannelInfo::resolve(name, tint, view.format, &[probe], &[probe])
}

fn format_of(pixel: PixelType) -> Option<VoxelFormat> {
    match pixel {
        PixelType::Uint8 => Some(VoxelFormat::U8),
        PixelType::Uint16 => Some(VoxelFormat::U16),
        PixelType::Float => Some(VoxelFormat::F32),
        PixelType::Int8
        | PixelType::Int16
        | PixelType::Uint32
        | PixelType::Int32
        | PixelType::Double => Some(VoxelFormat::F32),
        _ => None,
    }
}

fn spacing(image: &ImageInfo) -> (Spacing, Axial) {
    let x = positive(image.physical_size.x).unwrap_or(1.0);
    let y = positive(image.physical_size.y).unwrap_or(x);
    let Some(z) = positive(image.physical_size.z) else {
        let axial = if image.size_z > 1 {
            Axial::Assumed
        } else {
            Axial::Recorded
        };
        return (Spacing::new(x, y, x), axial);
    };
    (Spacing::new(x, y, z), Axial::Recorded)
}

fn positive(value: Option<f64>) -> Option<f32> {
    value
        .map(|value| value as f32)
        .filter(|value| value.is_finite() && *value > 0.0)
}

fn interval(image: &ImageInfo) -> f32 {
    positive(image.time_increment_s).unwrap_or(1.0)
}

fn color(value: &str) -> Option<[f32; 3]> {
    let hex = value.strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }
    let component = |range: Range<usize>| {
        u8::from_str_radix(&hex[range], 16)
            .ok()
            .map(|value| f32::from(value) / 255.0)
    };
    Some([component(0..2)?, component(2..4)?, component(4..6)?])
}
