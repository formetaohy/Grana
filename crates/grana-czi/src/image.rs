use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use grana_volume::{
    Census, ChannelInfo, Extent, Intensity, Level, Measured, Pyramid, Statistics, VoxelData,
    VoxelFormat, VoxelSemantics, name_of, tint_of,
};

use crate::Error;
use crate::binary::u32_at;
use crate::directory;
use crate::header::{self, Header};
use crate::layout::{Layout, LevelLayout, Plan, PlaneKey, SceneLayout};
use crate::metadata::{self, Metadata};
use crate::plane;
use crate::segment;
use crate::store::Store;

const METADATA_XML_OFFSET: u64 = 256;
const DISPLAY_LIMIT: f32 = 1.5;

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
    identity: u64,
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
        let identity = document.store.identity();
        let base = &selected.levels[0];
        let metadata = &document.metadata;
        let mut channels = Vec::with_capacity(selected.pixels.len());
        for (index, pixel) in selected.pixels.iter().enumerate() {
            let format = pixel.format();
            let display = metadata.display.get(index);
            let embedded = display.and_then(|display| window(display.low, display.high, format));
            let sampled = match embedded {
                Some(_) => None,
                None => Some(probe(
                    &document.store,
                    path,
                    identity,
                    base,
                    format,
                    index as u32,
                )?),
            };
            let statistics = match &sampled {
                Some(sampled) => sampled.statistics(),
                None => Statistics::declared(
                    embedded.expect("a channel either declares its window or is probed"),
                    VoxelSemantics::Scalar,
                ),
            };
            let source = metadata.channels.get(index);
            let name = display
                .and_then(|display| display.name.clone())
                .or_else(|| source.and_then(|channel| channel.name.clone()))
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| name_of(index as u32));
            let tint = source
                .and_then(|channel| channel.tint)
                .or_else(|| display.and_then(|display| display.tint))
                .unwrap_or_else(|| tint_of(index as u32));
            let info = ChannelInfo::new(name, tint, statistics, format);
            channels.push(match sampled {
                Some(sampled) => info.with_measured(sampled),
                None => info,
            });
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
            selected.calibration,
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
            identity,
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
        let bytes = region(
            &self.store,
            &self.path,
            self.identity,
            &level.plans,
            &request,
        )?;
        Ok(voxels(format, bytes))
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
    identity: u64,
    plans: &HashMap<PlaneKey, Plan>,
    request: &Request<'_>,
) -> Result<Vec<u8>, Error> {
    let sample = request.format.bytes();
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
            identity,
            plan,
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
    identity: u64,
    plan: &Plan,
    request: &Request<'_>,
    depth: u32,
    out: &mut [u8],
) -> Result<(), Error> {
    let sample = request.format.bytes();
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
        let tile_row = tile.width as usize * sample;
        let origin = held * tile_row * tile.height as usize;
        let samples = samples(store, path, identity, tile)?;
        let columns = (right - left) as usize * sample;
        for line in top..bottom {
            let source =
                origin + ((line - tile.y) as usize * tile_row + (left - tile.x) as usize * sample);
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
    identity: u64,
    tile: &plane::Tile,
) -> Result<Arc<Vec<u8>>, Error> {
    let cache = crate::cache();
    let key = (identity, tile.position);
    cache.load(key, || plane::samples(store, path, tile))
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

fn window(low: Option<f32>, high: Option<f32>, format: VoxelFormat) -> Option<Intensity> {
    let low = low.unwrap_or(0.0);
    let high = high?;
    if !(0.0..=DISPLAY_LIMIT).contains(&low) || !(0.0..=DISPLAY_LIMIT).contains(&high) {
        return None;
    }
    if high <= low {
        return None;
    }
    let top = format.display_maximum();
    Some(Intensity::new(low * top, high * top))
}

fn probe(
    store: &Store,
    path: &Path,
    identity: u64,
    level: &LevelLayout,
    format: VoxelFormat,
    channel: u32,
) -> Result<Measured, Error> {
    let extent = level.extent;
    let mut census = Census::of(extent, 1);
    let measured = census.measure(None, |time, ranges| {
        let request = Request {
            format,
            time,
            channel,
            x: &ranges[0],
            y: &ranges[1],
            z: &ranges[2],
        };
        region(store, path, identity, &level.plans, &request).map(|bytes| voxels(format, bytes))
    })?;
    Ok(measured)
}

fn voxels(format: VoxelFormat, bytes: Vec<u8>) -> VoxelData {
    VoxelData::reinterpret(format, &bytes)
}

pub fn dataset_name(path: &Path) -> String {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .unwrap_or("CZI")
        .to_string()
}
