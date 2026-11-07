use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use grana_volume::{
    BrickCoord, BrickData, BrickGrid, ChannelInfo, Extent, Intensity, Level, Pyramid, VoxelData,
    VoxelFormat, pad_brick,
};

use crate::Error;
use crate::binary::u32_at;
use crate::cache::Cache;
use crate::directory;
use crate::header::{self, Header};
use crate::layout::{Layout, LevelLayout, Plan, PlaneKey, SceneLayout};
use crate::metadata::{self, Metadata};
use crate::plane;
use crate::segment;
use crate::store::Store;

const CACHE_BYTES: u64 = 256 << 20;
const METADATA_XML_OFFSET: u64 = 256;
const PROBE_SIDE: u32 = 64;
const DISPLAY_LIMIT: f32 = 1.5;
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scene {
    extent: Extent,
    channels: usize,
    frames: u32,
    name: Option<String>,
}

impl Scene {
    fn new(extent: Extent, channels: usize, frames: u32, name: Option<String>) -> Self {
        Self {
            extent,
            channels,
            frames,
            name,
        }
    }

    pub fn extent(&self) -> Extent {
        self.extent
    }

    pub fn channels(&self) -> usize {
        self.channels
    }

    pub fn frames(&self) -> u32 {
        self.frames
    }

    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }
}

pub struct Image {
    path: PathBuf,
    name: String,
    scene: usize,
    scenes: Vec<Scene>,
    layouts: Vec<SceneLayout>,
    pyramid: Pyramid,
    store: Store,
    cache: Cache,
}

struct Document {
    store: Store,
    metadata: Metadata,
    scenes: Vec<SceneLayout>,
    interval: f32,
}

impl Document {
    fn read(path: &Path) -> Result<Self, Error> {
        let store = Store::open(path)?;
        let header = header::read(&store, path)?;
        let metadata = xml(&store, &header, path)?;
        let entries = directory::read(&store, header.directory, path)?;
        let layout = Layout::of(entries, &metadata, path)?;
        Ok(Self {
            store,
            metadata,
            scenes: layout.scenes,
            interval: layout.interval,
        })
    }

    fn scenes(&self) -> Vec<Scene> {
        self.scenes
            .iter()
            .map(|scene| {
                Scene::new(
                    scene.levels[0].extent,
                    scene.pixels.len(),
                    scene.frames,
                    scene.name.clone(),
                )
            })
            .collect()
    }
}

pub fn scenes(path: &Path) -> Result<Vec<Scene>, Error> {
    Ok(Document::read(path)?.scenes())
}

pub fn scene_count(path: &Path) -> Result<usize, Error> {
    let store = Store::open(path)?;
    let header = header::read(&store, path)?;
    let entries = directory::read(&store, header.directory, path)?;
    crate::layout::scene_count(entries, path)
}

impl Image {
    pub fn open(path: &Path, scene: usize) -> Result<Self, Error> {
        let document = Document::read(path)?;
        let selected = document.scenes.get(scene).ok_or_else(|| {
            Error::region(
                path,
                format!(
                    "the CZI image holds {} scenes and scene {} was requested",
                    document.scenes.len(),
                    scene + 1
                ),
            )
        })?;
        let cache = Cache::new(CACHE_BYTES);
        let base = &selected.levels[0];
        let metadata = &document.metadata;
        let mut channels = Vec::with_capacity(selected.pixels.len());
        for (index, pixel) in selected.pixels.iter().enumerate() {
            let format = pixel.format();
            let intensity = intensity(format);
            let display = metadata.display.get(index);
            let window = display
                .and_then(|display| window(display.low, display.high, intensity))
                .or_else(|| probe(&document.store, path, base, &cache, format, index as u32))
                .filter(|window| window.span() > 0.0)
                .unwrap_or(intensity);
            let source = metadata.channels.get(index);
            let name = display
                .and_then(|display| display.name.clone())
                .or_else(|| source.and_then(|channel| channel.name.clone()))
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| format!("Channel {}", index + 1));
            let tint = source
                .and_then(|channel| channel.tint)
                .or_else(|| display.and_then(|display| display.tint))
                .unwrap_or(PALETTE[index % PALETTE.len()]);
            channels.push(ChannelInfo::new(name, tint, intensity, window, format));
        }
        let stored = selected.levels.len();
        let pyramid = Pyramid::new(
            selected
                .levels
                .iter()
                .map(|level| Level::new(level.extent, level.spacing))
                .collect(),
            stored,
            channels,
            selected.frames,
            document.interval,
        );
        let scenes = document.scenes();
        Ok(Self {
            path: path.to_path_buf(),
            name: dataset_name(path),
            scene,
            scenes,
            layouts: document.scenes,
            pyramid,
            store: document.store,
            cache,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn scene(&self) -> usize {
        self.scene
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
        self.validate(time, level, channel, &x, &y, &z)?;
        let level = &self.levels()[level];
        let channel = u32::try_from(channel)
            .map_err(|_| Error::region(&self.path, "the requested channel is out of range"))?;
        let format = self.pyramid.channels()[channel as usize].format();
        let request = Request {
            format,
            time,
            channel,
            x: &x,
            y: &y,
            z: &z,
        };
        let bytes = region(&self.store, &self.path, &level.plans, &self.cache, &request)?;
        Ok(voxels(format, bytes))
    }

    pub fn read_brick(
        &self,
        time: u32,
        level: usize,
        coord: BrickCoord,
    ) -> Result<BrickData, Error> {
        let Some(target) = self.levels().get(level) else {
            return Err(Error::region(
                &self.path,
                "the requested resolution level does not exist",
            ));
        };
        let grid = BrickGrid::new(target.extent);
        let spatial = grid.padded_range(coord);
        let origin = [spatial[0].start, spatial[1].start, spatial[2].start];
        let extent = Extent::new(
            spatial[0].end - spatial[0].start,
            spatial[1].end - spatial[1].start,
            spatial[2].end - spatial[2].start,
        );
        let mut channels = Vec::with_capacity(self.pyramid.channel_count());
        for channel in 0..self.pyramid.channel_count() {
            let data = self.read_region(
                time,
                level,
                channel,
                spatial[0].clone(),
                spatial[1].clone(),
                spatial[2].clone(),
            )?;
            channels.push(pad_brick(
                &data,
                origin,
                extent,
                grid.origin(coord),
                target.extent,
            ));
        }
        Ok(BrickData::new(channels))
    }

    fn levels(&self) -> &[LevelLayout] {
        &self.layouts[self.scene].levels
    }

    fn validate(
        &self,
        time: u32,
        level: usize,
        channel: usize,
        x: &Range<u32>,
        y: &Range<u32>,
        z: &Range<u32>,
    ) -> Result<(), Error> {
        if level >= self.levels().len() {
            return Err(Error::region(
                &self.path,
                "the requested resolution level does not exist",
            ));
        }
        if time >= self.pyramid.frame_count() {
            return Err(Error::region(
                &self.path,
                "the requested frame does not exist",
            ));
        }
        if channel >= self.pyramid.channel_count() {
            return Err(Error::region(
                &self.path,
                "the requested channel does not exist",
            ));
        }
        let extent = self.levels()[level].extent;
        let inside = x.start < x.end
            && x.end <= extent.x()
            && y.start < y.end
            && y.end <= extent.y()
            && z.start < z.end
            && z.end <= extent.z();
        if !inside {
            return Err(Error::region(
                &self.path,
                "the region lies outside the image extent",
            ));
        }
        Ok(())
    }
}

struct Request<'a> {
    format: VoxelFormat,
    time: u32,
    channel: u32,
    x: &'a Range<u32>,
    y: &'a Range<u32>,
    z: &'a Range<u32>,
}

fn region(
    store: &Store,
    path: &Path,
    plans: &HashMap<PlaneKey, Plan>,
    cache: &Cache,
    request: &Request<'_>,
) -> Result<Vec<u8>, Error> {
    let sample = sample_bytes(request.format);
    let width = (request.x.end - request.x.start) as usize;
    let height = (request.y.end - request.y.start) as usize;
    let row = width * sample;
    let mut bytes = vec![0u8; row * height * (request.z.end - request.z.start) as usize];
    for depth in request.z.clone() {
        let plan = plans
            .get(&(request.time, request.channel, depth))
            .ok_or_else(|| Error::region(path, "the CZI image has no such plane"))?;
        let target = (depth - request.z.start) as usize * row * height;
        blit(
            store,
            path,
            plan,
            cache,
            request,
            depth,
            &mut bytes[target..target + row * height],
        )?;
    }
    Ok(bytes)
}

fn blit(
    store: &Store,
    path: &Path,
    plan: &Plan,
    cache: &Cache,
    request: &Request<'_>,
    depth: u32,
    out: &mut [u8],
) -> Result<(), Error> {
    let sample = sample_bytes(request.format);
    let width = (request.x.end - request.x.start) as usize;
    let row = width * sample;
    for tile in plan.tiles() {
        let left = request.x.start.max(tile.x);
        let right = request.x.end.min(tile.x + tile.width);
        let top = request.y.start.max(tile.y);
        let bottom = request.y.end.min(tile.y + tile.height);
        if left >= right || top >= bottom {
            continue;
        }
        if depth < tile.plane || depth >= tile.plane + tile.depth {
            continue;
        }
        let held = (depth - tile.plane) as usize;
        let origin = held * row * tile.height as usize;
        let samples = samples(store, path, cache, tile)?;
        let columns = (right - left) as usize * sample;
        for line in top..bottom {
            let source = origin
                + (((line - tile.y) as usize) * tile.width as usize + (left - tile.x) as usize)
                    * sample;
            let target = ((line - request.y.start) as usize) * row
                + (left - request.x.start) as usize * sample;
            out[target..target + columns].copy_from_slice(&samples[source..source + columns]);
        }
    }
    Ok(())
}

fn samples(
    store: &Store,
    path: &Path,
    cache: &Cache,
    tile: &plane::Tile,
) -> Result<Arc<Vec<u8>>, Error> {
    if let Some(samples) = cache.lookup(tile.position) {
        return Ok(samples);
    }
    let samples = Arc::new(plane::samples(store, path, tile)?);
    cache.insert(tile.position, Arc::clone(&samples));
    Ok(samples)
}

fn xml(store: &Store, header: &Header, path: &Path) -> Result<Metadata, Error> {
    if header.metadata == 0 {
        return Ok(Metadata::default());
    }
    let segment = segment::read(store, header.metadata, path)?;
    if !segment.is(b"ZISRAWMETADATA") {
        return Err(Error::metadata(
            path,
            "the file header does not point at a metadata segment",
        ));
    }
    let sizes = store.read(segment.data_offset(), 8)?;
    let size = u32_at(&sizes, 0)
        .and_then(|size| usize::try_from(size).ok())
        .filter(|size| *size > 0)
        .ok_or_else(|| Error::metadata(path, "the metadata segment holds no XML"))?;
    let text = store.read(segment.data_offset() + METADATA_XML_OFFSET, size)?;
    metadata::parse(&String::from_utf8_lossy(&text)).map_err(|detail| Error::metadata(path, detail))
}

fn window(low: Option<f32>, high: Option<f32>, intensity: Intensity) -> Option<Intensity> {
    let low = low.unwrap_or(0.0);
    let high = high?;
    if !(0.0..=DISPLAY_LIMIT).contains(&low) || !(0.0..=DISPLAY_LIMIT).contains(&high) {
        return None;
    }
    if high <= low {
        return None;
    }
    let top = intensity.max();
    Some(Intensity::new(low * top, high * top))
}

fn intensity(format: VoxelFormat) -> Intensity {
    match format {
        VoxelFormat::U8 => Intensity::new(0.0, f32::from(u8::MAX)),
        VoxelFormat::U16 => Intensity::new(0.0, f32::from(u16::MAX)),
        VoxelFormat::F32 => Intensity::new(0.0, 1.0),
    }
}

fn sample_bytes(format: VoxelFormat) -> usize {
    match format {
        VoxelFormat::U8 => 1,
        VoxelFormat::U16 => 2,
        VoxelFormat::F32 => 4,
    }
}

fn probe(
    store: &Store,
    path: &Path,
    level: &LevelLayout,
    cache: &Cache,
    format: VoxelFormat,
    channel: u32,
) -> Option<Intensity> {
    let extent = level.extent;
    let side_x = extent.x().min(PROBE_SIDE);
    let side_y = extent.y().min(PROBE_SIDE);
    let left = (extent.x() - side_x) / 2;
    let top = (extent.y() - side_y) / 2;
    let x = left..left + side_x;
    let y = top..top + side_y;
    let depth = extent.z() / 2;
    let z = depth..depth + 1;
    let request = Request {
        format,
        time: 0,
        channel,
        x: &x,
        y: &y,
        z: &z,
    };
    let bytes = region(store, path, &level.plans, cache, &request).ok()?;
    let sample = sample_bytes(format);
    let mut range = (f32::INFINITY, f32::NEG_INFINITY);
    for line in 0..side_y as usize {
        let start = line * side_x as usize * sample;
        let (low, high) = samples_range(format, &bytes[start..start + side_x as usize * sample]);
        range = (range.0.min(low), range.1.max(high));
    }
    (range.1 > range.0).then(|| Intensity::new(range.0, range.1))
}

fn samples_range(format: VoxelFormat, row: &[u8]) -> (f32, f32) {
    match format {
        VoxelFormat::U8 => {
            let low = row.iter().copied().min().unwrap_or(0);
            let high = row.iter().copied().max().unwrap_or(0);
            (f32::from(low), f32::from(high))
        }
        VoxelFormat::U16 => {
            let mut low = u16::MAX;
            let mut high = 0;
            let (samples, _) = row.as_chunks::<2>();
            for sample in samples {
                let value = u16::from_le_bytes(*sample);
                low = low.min(value);
                high = high.max(value);
            }
            (f32::from(low), f32::from(high))
        }
        VoxelFormat::F32 => {
            let mut low = f32::INFINITY;
            let mut high = f32::NEG_INFINITY;
            let (samples, _) = row.as_chunks::<4>();
            for sample in samples {
                let value = f32::from_le_bytes(*sample);
                if value.is_finite() {
                    low = low.min(value);
                    high = high.max(value);
                }
            }
            (low, high)
        }
    }
}

fn voxels(format: VoxelFormat, bytes: Vec<u8>) -> VoxelData {
    match format {
        VoxelFormat::U8 => VoxelData::U8(bytes),
        VoxelFormat::U16 => {
            let (samples, _) = bytes.as_chunks::<2>();
            VoxelData::U16(
                samples
                    .iter()
                    .map(|sample| u16::from_le_bytes(*sample))
                    .collect(),
            )
        }
        VoxelFormat::F32 => {
            let (samples, _) = bytes.as_chunks::<4>();
            VoxelData::F32(
                samples
                    .iter()
                    .map(|sample| f32::from_le_bytes(*sample))
                    .collect(),
            )
        }
    }
}

pub fn dataset_name(path: &Path) -> String {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .unwrap_or("CZI")
        .to_string()
}
