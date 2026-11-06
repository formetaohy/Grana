use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use grana_volume::{
    BrickCoord, BrickData, BrickGrid, ChannelInfo, Extent, Intensity, Level, Pyramid, Spacing,
    VoxelData, VoxelFormat, pad_brick,
};
use openreadout_core::{Dataset, ImageInfo, PixelType, Plane, PlaneIndex};

use crate::error::Error;
use crate::family::Family;
use crate::open;
use crate::plane::{self, View};
use crate::probe;
use crate::scene::{self, Scene};

const CACHE_BYTES: usize = 256 << 20;
const DISPLAY_FORMAT: f32 = 65535.0;

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

const RED: [f32; 3] = [1.0, 0.0, 0.0];
const GREEN: [f32; 3] = [0.0, 1.0, 0.0];
const BLUE: [f32; 3] = [0.0, 0.0, 1.0];

pub struct Image {
    path: PathBuf,
    family: Family,
    name: String,
    index: u32,
    scenes: Vec<Scene>,
    extent: Extent,
    frames: u32,
    pixel: PixelType,
    samples: u32,
    views: Vec<View>,
    pyramid: Pyramid,
    dataset: Mutex<Box<dyn Dataset>>,
    cache: Mutex<plane::Cache>,
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
        let extent = scene.extent();
        let frames = scene.frames();
        let spacing = spacing(image);
        let interval = interval(image);
        let mut dataset = document.dataset;
        let mut cache = plane::Cache::new(CACHE_BYTES);
        let probes = probe::intensities(path, &mut *dataset, image, &views, &mut cache)?;
        let channels = views
            .iter()
            .enumerate()
            .map(|(index, view)| channel(image, *view, index, probes[index]))
            .collect();
        let pyramid = Pyramid::new(
            vec![Level::new(extent, spacing)],
            1,
            channels,
            frames,
            interval,
        );
        Ok(Self {
            path: path.to_path_buf(),
            family: document.family,
            name: scene
                .name()
                .map(str::to_string)
                .unwrap_or_else(|| scene::dataset_name(path)),
            index: image.index,
            scenes,
            extent,
            frames,
            pixel: image.pixel_type,
            samples,
            views,
            pyramid,
            dataset: Mutex::new(dataset),
            cache: Mutex::new(cache),
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
        if level != 0 {
            return Err(Error::unsupported(
                &self.path,
                "the microscope image stores a single resolution level",
            ));
        }
        if time >= self.frames {
            return Err(Error::region(
                &self.path,
                "the requested frame lies outside the image",
            ));
        }
        let view = *self.views.get(channel).ok_or_else(|| {
            Error::region(&self.path, "the requested channel lies outside the image")
        })?;
        self.clamp(&x, &y, &z)?;
        let width = (x.end - x.start) as usize;
        let height = (y.end - y.start) as usize;
        let depth = (z.end - z.start) as usize;
        let mut data = plane::allocate(view.format, width * height * depth);
        let slice = width * height;
        for index in 0..depth {
            let position = z.start + index as u32;
            let plane = self.plane(time, view, position)?;
            plane::copy(&plane, view.sample, &x, &y, index * slice, &mut data);
        }
        Ok(data)
    }

    pub fn read_brick(
        &self,
        time: u32,
        level: usize,
        coord: BrickCoord,
    ) -> Result<BrickData, Error> {
        if level != 0 {
            return Err(Error::unsupported(
                &self.path,
                "the microscope image stores a single resolution level",
            ));
        }
        let grid = BrickGrid::new(self.extent);
        let counts = grid.counts();
        if coord.x() >= counts[0] || coord.y() >= counts[1] || coord.z() >= counts[2] {
            return Err(Error::region(
                &self.path,
                "the requested brick lies outside the image",
            ));
        }
        let spatial = grid.padded_range(coord);
        let origin = [spatial[0].start, spatial[1].start, spatial[2].start];
        let covered = Extent::new(
            spatial[0].end - spatial[0].start,
            spatial[1].end - spatial[1].start,
            spatial[2].end - spatial[2].start,
        );
        let mut bricks = Vec::with_capacity(self.views.len());
        for channel in 0..self.views.len() {
            let data = self.read_region(
                time,
                level,
                channel,
                spatial[0].clone(),
                spatial[1].clone(),
                spatial[2].clone(),
            )?;
            bricks.push(pad_brick(
                &data,
                origin,
                covered,
                grid.origin(coord),
                self.extent,
            ));
        }
        Ok(BrickData::new(bricks))
    }

    fn clamp(&self, x: &Range<u32>, y: &Range<u32>, z: &Range<u32>) -> Result<(), Error> {
        if x.end <= x.start || y.end <= y.start || z.end <= z.start {
            return Err(Error::region(&self.path, "a requested region is empty"));
        }
        if x.end > self.extent.x() || y.end > self.extent.y() || z.end > self.extent.z() {
            return Err(Error::region(
                &self.path,
                "the requested region leaves the image",
            ));
        }
        Ok(())
    }

    fn plane(&self, time: u32, view: View, z: u32) -> Result<Arc<Plane>, Error> {
        let key = (time, view.channel, z);
        let cached = self
            .cache
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(&key);
        if let Some(plane) = cached {
            return Ok(plane);
        }
        let plane = {
            let mut dataset = self
                .dataset
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            dataset
                .read_plane(
                    self.index,
                    PlaneIndex {
                        c: view.channel,
                        z,
                        t: time,
                    },
                )
                .map_err(|error| Error::read(&self.path, error.to_string()))?
        };
        if plane.pixel_type != self.pixel || plane.samples_per_pixel != self.samples {
            return Err(Error::read(
                &self.path,
                "a plane disagrees with the declared sample type",
            ));
        }
        if plane.width != self.extent.x() || plane.height != self.extent.y() {
            return Err(Error::read(
                &self.path,
                "a plane disagrees with the declared extent",
            ));
        }
        if plane.data.len() != plane.expected_len() {
            return Err(Error::read(&self.path, "a plane holds an unexpected size"));
        }
        let plane = Arc::new(plane);
        self.cache
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .put(key, Arc::clone(&plane), plane.data.len());
        Ok(plane)
    }
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
        .map_or_else(|| format!("Channel {}", view.channel + 1), str::to_string);
    let (name, tint) = if image.samples_per_pixel <= 1 {
        let tint = source
            .and_then(|channel| channel.color.as_deref())
            .and_then(color)
            .unwrap_or(PALETTE[index % PALETTE.len()]);
        (base, tint)
    } else {
        let (suffix, tint) = match view.sample {
            0 => ("Red", RED),
            1 => ("Green", GREEN),
            _ => ("Blue", BLUE),
        };
        (format!("{base} {suffix}"), tint)
    };
    let intensity = match view.format {
        VoxelFormat::U8 => Intensity::new(0.0, 255.0),
        VoxelFormat::U16 => Intensity::new(0.0, DISPLAY_FORMAT),
        VoxelFormat::F32 => probe.unwrap_or_else(|| Intensity::new(0.0, 1.0)),
    };
    let window = probe
        .filter(|range| range.span() > 0.0)
        .unwrap_or(intensity);
    let window = if window.span() > 0.0 {
        window
    } else {
        Intensity::new(0.0, 1.0)
    };
    ChannelInfo::new(name, tint, intensity, window, view.format)
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

fn spacing(image: &ImageInfo) -> Spacing {
    let x = positive(image.physical_size.x).unwrap_or(1.0);
    Spacing::new(
        x,
        positive(image.physical_size.y).unwrap_or(x),
        positive(image.physical_size.z).unwrap_or(x),
    )
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
