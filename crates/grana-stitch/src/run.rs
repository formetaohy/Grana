use std::path::PathBuf;

use grana_image::{Image, Source};
use grana_volume::{Extent, Level, Spacing};
use grana_zarr::{ImageSpec, WriteChannel, write_image};

use crate::Error;
use crate::mosaic::{self, Edge, Offset, Plane, Side};

const REGISTRATION_SIDE: u32 = 1024;
const FEATHER: f32 = 32.0;
const WEIGHT_FLOOR: f32 = 1.0e-3;

pub struct Request {
    pub tiles: Vec<Source>,
    pub output: PathBuf,
    pub rows: usize,
    pub columns: usize,
    pub channel: usize,
}

pub struct Summary {
    width: u32,
    height: u32,
    depth: u32,
    channels: usize,
    frames: u32,
    bytes: u64,
    offsets: Vec<[i64; 2]>,
}

impl Summary {
    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn depth(&self) -> u32 {
        self.depth
    }

    pub fn channels(&self) -> usize {
        self.channels
    }

    pub fn frames(&self) -> u32 {
        self.frames
    }

    pub fn bytes(&self) -> u64 {
        self.bytes
    }

    pub fn offsets(&self) -> &[[i64; 2]] {
        &self.offsets
    }
}

pub fn stitch(request: &Request) -> Result<Summary, Error> {
    let mosaic = Mosaic::new(request)?;
    let name = request.output.file_stem().map_or_else(
        || "stitched".to_string(),
        |stem| stem.to_string_lossy().into_owned(),
    );
    let levels = [mosaic.level()];
    let channels = mosaic.channels();
    let mut fill =
        |level: usize,
         time: u32,
         channel: usize,
         origin: [u32; 3],
         length: [u32; 3],
         out: &mut [f32]| { mosaic.fill(level, time, channel, origin, length, out) };
    let mut progress = |_: usize, _: usize| {};
    let written = write_image(
        &request.output,
        ImageSpec {
            name: &name,
            channels: &channels,
            levels: &levels,
            placement: mosaic.placement,
            frames: mosaic.frames,
            interval: mosaic.interval,
            fill: &mut fill,
            progress: &mut progress,
        },
    )
    .map_err(|error| Error::new(error.to_string()))?;
    Ok(Summary {
        width: mosaic.extent.x(),
        height: mosaic.extent.y(),
        depth: mosaic.extent.z(),
        channels: channels.len(),
        frames: mosaic.frames,
        bytes: written.bytes(),
        offsets: mosaic.offsets(),
    })
}

struct Mosaic {
    images: Vec<Image>,
    positions: Vec<Vec<Offset>>,
    min: Offset,
    tile: Extent,
    extent: Extent,
    spacing: Spacing,
    placement: [f32; 3],
    frames: u32,
    interval: f32,
}

impl Mosaic {
    fn new(request: &Request) -> Result<Self, Error> {
        if request.rows == 0 || request.columns == 0 {
            return Err(Error::new(
                "a tile grid holds at least one row and one column",
            ));
        }
        if request.rows * request.columns != request.tiles.len() {
            return Err(Error::new(format!(
                "the grid holds {} tiles and {} were given",
                request.rows * request.columns,
                request.tiles.len()
            )));
        }
        if request.tiles.len() < 2 {
            return Err(Error::new("a mosaic spans at least two tiles"));
        }
        let mut images = Vec::with_capacity(request.tiles.len());
        for source in &request.tiles {
            let image = Image::open_with_cache(source, None).map_err(|error| {
                Error::new(format!("failed to open {}: {error}", source.label()))
            })?;
            images.push(image);
        }
        let base = images[0].pyramid();
        let extent = base.level(0).extent();
        let spacing = base.level(0).spacing();
        let channels = base.channel_count();
        let frames = base.frame_count();
        if request.channel >= channels {
            return Err(Error::new(format!(
                "the tiles hold {channels} channels and channel {} was chosen",
                request.channel
            )));
        }
        for (index, image) in images.iter().enumerate() {
            let pyramid = image.pyramid();
            let path = request.tiles[index].label();
            if pyramid.channel_count() != channels {
                return Err(Error::new(format!(
                    "{path} holds {} channels and the first tile holds {channels}",
                    pyramid.channel_count()
                )));
            }
            if pyramid.frame_count() != frames {
                return Err(Error::new(format!(
                    "{path} holds {} frames and the first tile holds {frames}",
                    pyramid.frame_count()
                )));
            }
            let level = pyramid.level(0);
            if level.extent() != extent {
                return Err(Error::new(format!(
                    "{path} covers another extent than the first tile"
                )));
            }
            if level.spacing() != spacing {
                return Err(Error::new(format!(
                    "{path} carries another voxel spacing than the first tile"
                )));
            }
        }
        let mut registration = 0;
        while registration + 1 < base.level_count()
            && (base.level(registration).extent().x() > REGISTRATION_SIDE
                || base.level(registration).extent().y() > REGISTRATION_SIDE)
        {
            registration += 1;
        }
        let coarse = base.level(registration).extent();
        let scale = [
            f64::from(extent.x()) / f64::from(coarse.x()),
            f64::from(extent.y()) / f64::from(coarse.y()),
        ];
        let placement = base.placement();
        let interval = base.frame_interval();
        let mut positions = Vec::with_capacity(frames as usize);
        for time in 0..frames {
            let planes = (0..images.len())
                .map(|index| plane(&images[index], time, registration, request.channel))
                .collect::<Result<Vec<_>, Error>>()?;
            let mut edges = Vec::new();
            for row in 0..request.rows {
                for column in 0..request.columns {
                    let tile = row * request.columns + column;
                    if column + 1 < request.columns {
                        let next = tile + 1;
                        let offset = mosaic::estimate(&planes[tile], &planes[next], Side::Right)
                            .map_err(|error| {
                                Error::new(format!(
                                    "the tiles {} and {} do not register: {error}",
                                    tile + 1,
                                    next + 1
                                ))
                            })?;
                        edges.push(Edge::new(tile, next, offset));
                    }
                    if row + 1 < request.rows {
                        let next = tile + request.columns;
                        let offset = mosaic::estimate(&planes[tile], &planes[next], Side::Below)
                            .map_err(|error| {
                                Error::new(format!(
                                    "the tiles {} and {} do not register: {error}",
                                    tile + 1,
                                    next + 1
                                ))
                            })?;
                        edges.push(Edge::new(tile, next, offset));
                    }
                }
            }
            let solved = mosaic::solve(images.len(), &edges)?;
            positions.push(
                solved
                    .iter()
                    .map(|offset| {
                        Offset::new(
                            (offset.x as f64 * scale[0]).round() as i64,
                            (offset.y as f64 * scale[1]).round() as i64,
                        )
                    })
                    .collect::<Vec<_>>(),
            );
        }
        let mut min = Offset::new(i64::MAX, i64::MAX);
        let mut max = Offset::new(i64::MIN, i64::MIN);
        for positions in &positions {
            for offset in positions {
                min.x = min.x.min(offset.x);
                min.y = min.y.min(offset.y);
                max.x = max.x.max(offset.x + i64::from(extent.x()));
                max.y = max.y.max(offset.y + i64::from(extent.y()));
            }
        }
        let width = u32::try_from(max.x - min.x)
            .map_err(|_| Error::new("the mosaic is wider than the image format holds"))?;
        let height = u32::try_from(max.y - min.y)
            .map_err(|_| Error::new("the mosaic is taller than the image format holds"))?;
        let canvas = Extent::new(width, height, extent.z());
        let origin = positions[0][0];
        let shift = [
            spacing.x()
                * (0.5 * width as f32 - (origin.x - min.x) as f32 - 0.5 * extent.x() as f32),
            spacing.y()
                * (0.5 * height as f32 - (origin.y - min.y) as f32 - 0.5 * extent.y() as f32),
            0.0,
        ];
        let placement = [
            placement[0] + shift[0],
            placement[1] + shift[1],
            placement[2],
        ];
        Ok(Self {
            images,
            positions,
            min,
            tile: extent,
            extent: canvas,
            spacing,
            placement,
            frames,
            interval,
        })
    }

    fn level(&self) -> Level {
        Level::new(self.extent, self.spacing)
    }

    fn channels(&self) -> Vec<WriteChannel<'_>> {
        self.images[0]
            .pyramid()
            .channels()
            .iter()
            .map(|channel| WriteChannel {
                name: channel.name(),
                tint: channel.tint(),
                range: channel.intensity(),
            })
            .collect()
    }

    fn offsets(&self) -> Vec<[i64; 2]> {
        self.positions[0]
            .iter()
            .map(|offset| [offset.x - self.min.x, offset.y - self.min.y])
            .collect()
    }

    fn fill(
        &self,
        level: usize,
        time: u32,
        channel: usize,
        origin: [u32; 3],
        length: [u32; 3],
        out: &mut [f32],
    ) -> Result<(), String> {
        if level != 0 {
            return Err("the stitched image holds a single level".to_string());
        }
        if out.len() != (length[0] * length[1] * length[2]) as usize {
            return Err("the stitched chunk does not cover its region".to_string());
        }
        let width = length[0] as usize;
        let height = length[1] as usize;
        let mut weights = vec![0.0f32; out.len()];
        let positions = &self.positions[time as usize];
        for (index, image) in self.images.iter().enumerate() {
            let base = [
                positions[index].x - self.min.x,
                positions[index].y - self.min.y,
            ];
            let mut ranges: [std::ops::Range<u32>; 3] = std::array::from_fn(|_| 0..0);
            let mut spans = [0u32; 3];
            let mut covered = true;
            for axis in 0..2 {
                let extent = if axis == 0 {
                    self.tile.x()
                } else {
                    self.tile.y()
                };
                let low = (i64::from(origin[axis]) - base[axis]).max(0);
                let high = (i64::from(origin[axis]) + i64::from(length[axis]) - base[axis])
                    .min(i64::from(extent));
                if high <= low {
                    covered = false;
                    break;
                }
                ranges[axis] = low as u32..high as u32;
                spans[axis] = (high - low) as u32;
            }
            if !covered {
                continue;
            }
            ranges[2] = origin[2]..origin[2] + length[2];
            spans[2] = length[2];
            let data = image
                .read_region(time, 0, channel, &ranges, [0.0; 3])
                .map_err(|error| error.to_string())?;
            let axes = [self.tile.x(), self.tile.y(), self.tile.z()];
            let feather = axes.map(|axis| (axis as f32 / 8.0).clamp(1.0, FEATHER));
            for z in 0..spans[2] {
                for y in 0..spans[1] {
                    for x in 0..spans[0] {
                        let local = [
                            ranges[0].start + x,
                            ranges[1].start + y,
                            ranges[2].start + z,
                        ];
                        let weight = weight(local, axes, feather);
                        let target = [
                            (i64::from(local[0]) + base[0] - i64::from(origin[0])) as usize,
                            (i64::from(local[1]) + base[1] - i64::from(origin[1])) as usize,
                            z as usize,
                        ];
                        let slot = target[0] + width * (target[1] + height * target[2]);
                        let source = (x + spans[0] * (y + spans[1] * z)) as usize;
                        out[slot] += data.value(source) * weight;
                        weights[slot] += weight;
                    }
                }
            }
        }
        for (value, weight) in out.iter_mut().zip(&weights) {
            if *weight > 0.0 {
                *value /= *weight;
            } else {
                *value = 0.0;
            }
        }
        Ok(())
    }
}

fn plane(image: &Image, time: u32, level: usize, channel: usize) -> Result<Plane, Error> {
    let extent = image.pyramid().level(level).extent();
    let ranges = [0..extent.x(), 0..extent.y(), 0..extent.z()];
    let data = image
        .read_region(time, level, channel, &ranges, [0.0; 3])
        .map_err(|error| Error::new(error.to_string()))?;
    let pixels = (extent.x() * extent.y()) as usize;
    let mut values = vec![f32::NEG_INFINITY; pixels];
    for slab in 0..extent.z() as usize {
        for (index, slot) in values.iter_mut().enumerate() {
            let value = data.value(slab * pixels + index);
            if value > *slot {
                *slot = value;
            }
        }
    }
    Ok(Plane::new(extent.x() as usize, extent.y() as usize, values))
}

fn weight(local: [u32; 3], extent: [u32; 3], feather: [f32; 3]) -> f32 {
    let mut weight = 1.0f32;
    for axis in 0..3 {
        let low = (local[axis] as f32 + 0.5) / feather[axis];
        let high = (extent[axis] as f32 - local[axis] as f32 - 0.5) / feather[axis];
        weight *= low.min(high).clamp(WEIGHT_FLOOR, 1.0);
    }
    weight
}
