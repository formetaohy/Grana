use std::path::PathBuf;

use grana_archive::{Channel as WriteChannel, ImageSpec};
use grana_image::{Image, Source};
use grana_volume::{Extent, Intensity, Level, Spacing};

use crate::Error;
use crate::mosaic::{self, Edge, Offset, Plane, Side};

const REGISTRATION_SIDE: u32 = 1024;
const FEATHER: f32 = 32.0;
const WEIGHT_FLOOR: f32 = 1.0e-3;
const BLOCK: u32 = 64;
const LEVEL_LIMIT: usize = 6;
const LEVEL_FLOOR: u32 = 32;

pub struct Request {
    pub tiles: Vec<Source>,
    pub output: PathBuf,
    pub rows: usize,
    pub columns: usize,
    pub channel: usize,
    pub series: usize,
}

pub struct Summary {
    width: u32,
    height: u32,
    depth: u32,
    channels: usize,
    frames: u32,
    series: usize,
    levels: usize,
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

    pub fn series(&self) -> usize {
        self.series
    }

    pub fn levels(&self) -> usize {
        self.levels
    }

    pub fn bytes(&self) -> u64 {
        self.bytes
    }

    pub fn offsets(&self) -> &[[i64; 2]] {
        &self.offsets
    }
}

pub fn series_count(tiles: &[Source]) -> Result<usize, Error> {
    if tiles.is_empty() {
        return Err(Error::new("a mosaic holds at least one tile"));
    }
    let mut count = None;
    for source in tiles {
        let image = Image::open_with_cache(source, None)
            .map_err(|error| Error::new(format!("failed to open {}: {error}", source.label())))?;
        let scened = image.series().len();
        match count {
            None => count = Some(scened),
            Some(known) if known == scened => {}
            Some(known) => {
                return Err(Error::new(format!(
                    "{} holds {scened} series and the first tile holds {known}",
                    source.label()
                )));
            }
        }
    }
    Ok(count.expect("a mosaic holds a tile"))
}

pub fn stitch(request: &Request) -> Result<Summary, Error> {
    let mosaic = Mosaic::new(request)?;
    let name = request.output.file_stem().map_or_else(
        || "stitched".to_string(),
        |stem| stem.to_string_lossy().into_owned(),
    );
    let levels = mosaic.levels();
    let channels = mosaic.channels();
    let mut fill =
        |level: usize,
         time: u32,
         channel: usize,
         origin: [u32; 3],
         length: [u32; 3],
         out: &mut [f32]| { mosaic.fill(level, time, channel, origin, length, out) };
    let mut progress = |_: usize, _: usize| {};
    let mut image = ImageSpec {
        name: &name,
        channels: &channels,
        levels: &levels,
        placement: mosaic.placement,
        frames: mosaic.frames,
        interval: mosaic.interval,
        fill: &mut fill,
        progress: &mut progress,
    };
    let written = grana_zarr::write_image(&request.output, &mut image)
        .map_err(|error| Error::new(error.to_string()))?;
    Ok(Summary {
        width: mosaic.extent.x(),
        height: mosaic.extent.y(),
        depth: mosaic.extent.z(),
        channels: channels.len(),
        frames: mosaic.frames,
        series: request.series,
        levels: levels.len(),
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
            let source = at_series(source, request.series);
            let image = Image::open_with_cache(&source, None).map_err(|error| {
                Error::new(format!("failed to open {}: {error}", source.label()))
            })?;
            if image.selected() != request.series {
                return Err(Error::new(format!(
                    "{} holds {} series and series {} was chosen",
                    source.label(),
                    image.series().len(),
                    request.series
                )));
            }
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
            for (channel, (held, first)) in
                pyramid.channels().iter().zip(base.channels()).enumerate()
            {
                if held.name() != first.name() {
                    return Err(Error::new(format!(
                        "{path} names channel {} another way than the first tile",
                        channel + 1
                    )));
                }
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
        let mut solved = Vec::with_capacity(frames as usize);
        let mut failure = None;
        for time in 0..frames {
            match frame_positions(&images, request, time, registration, scale) {
                Ok(positions) => solved.push(Some(positions)),
                Err(error) => {
                    if failure.is_none() {
                        failure = Some(error);
                    }
                    solved.push(None);
                }
            }
        }
        let positions = propagate(solved, failure)?;
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

    fn levels(&self) -> Vec<Level> {
        let mut levels = vec![Level::new(self.extent, self.spacing)];
        while levels.len() < LEVEL_LIMIT {
            let coarser = levels[levels.len() - 1].coarser();
            let extent = coarser.extent();
            if extent.max_dimension() < LEVEL_FLOOR || extent == levels[levels.len() - 1].extent() {
                break;
            }
            levels.push(coarser);
        }
        levels
    }

    fn channels(&self) -> Vec<WriteChannel<'_>> {
        let base = self.images[0].pyramid().channels();
        (0..base.len())
            .map(|channel| {
                let window = self
                    .images
                    .iter()
                    .map(|image| image.pyramid().channels()[channel].intensity())
                    .reduce(|held, value| {
                        Intensity::new(held.min().min(value.min()), held.max().max(value.max()))
                    })
                    .expect("a mosaic holds a tile");
                WriteChannel {
                    name: base[channel].name(),
                    tint: base[channel].tint(),
                    range: window,
                }
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
        let voxels = (length[0] * length[1] * length[2]) as usize;
        if out.len() != voxels {
            return Err("the stitched chunk does not cover its region".to_string());
        }
        if level == 0 {
            return self.fill_base(time, channel, origin, length, out);
        }
        let factor = 1u32
            .checked_shl(level as u32)
            .ok_or_else(|| "the stitched level lies beyond the pyramid".to_string())?;
        let base = [0, 1, 2].map(|axis| {
            let limit = match axis {
                0 => self.extent.x(),
                1 => self.extent.y(),
                _ => self.extent.z(),
            };
            (
                (origin[axis] * factor).min(limit),
                ((origin[axis] + length[axis]) * factor).min(limit),
            )
        });
        out.fill(0.0);
        let mut counts = vec![0u32; out.len()];
        let step = BLOCK - BLOCK % factor;
        let mut scratch = vec![0.0f32; (step * step * step) as usize];
        let mut z = base[2].0;
        while z < base[2].1 {
            let depth = step.min(base[2].1 - z);
            let mut y = base[1].0;
            while y < base[1].1 {
                let rows = step.min(base[1].1 - y);
                let mut x = base[0].0;
                while x < base[0].1 {
                    let columns = step.min(base[0].1 - x);
                    let span = [columns, rows, depth];
                    let held = (columns * rows * depth) as usize;
                    self.fill_base(time, channel, [x, y, z], span, &mut scratch[..held])?;
                    for window in 0..depth {
                        for row in 0..rows {
                            for column in 0..columns {
                                let source = (column + columns * (row + rows * window)) as usize;
                                let target = [
                                    ((x + column) / factor - origin[0]) as usize,
                                    ((y + row) / factor - origin[1]) as usize,
                                    ((z + window) / factor - origin[2]) as usize,
                                ];
                                let slot = target[0]
                                    + length[0] as usize
                                        * (target[1] + length[1] as usize * target[2]);
                                out[slot] += scratch[source];
                                counts[slot] += 1;
                            }
                        }
                    }
                    x += step;
                }
                y += step;
            }
            z += step;
        }
        for (value, count) in out.iter_mut().zip(&counts) {
            *value = if *count > 0 {
                *value / *count as f32
            } else {
                0.0
            };
        }
        Ok(())
    }

    fn fill_base(
        &self,
        time: u32,
        channel: usize,
        origin: [u32; 3],
        length: [u32; 3],
        out: &mut [f32],
    ) -> Result<(), String> {
        let width = length[0] as usize;
        let height = length[1] as usize;
        out.fill(0.0);
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
                .read_region(time, 0, channel, &ranges, &grana_warp::Warp::IDENTITY)
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

fn at_series(source: &Source, series: usize) -> Source {
    match source {
        Source::File {
            path, group: None, ..
        } => Source::file_series(path.clone(), series),
        held => held.clone(),
    }
}

fn plane(image: &Image, time: u32, level: usize, channel: usize) -> Result<Plane, Error> {
    let extent = image.pyramid().level(level).extent();
    let ranges = [0..extent.x(), 0..extent.y(), 0..extent.z()];
    let data = image
        .read_region(time, level, channel, &ranges, &grana_warp::Warp::IDENTITY)
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

fn frame_positions(
    images: &[Image],
    request: &Request,
    time: u32,
    registration: usize,
    scale: [f64; 2],
) -> Result<Vec<Offset>, Error> {
    let planes = (0..images.len())
        .map(|index| plane(&images[index], time, registration, request.channel))
        .collect::<Result<Vec<_>, Error>>()?;
    if planes.iter().all(flat) {
        return Err(Error::new("the frame carries no signal to register"));
    }
    let mut edges = Vec::new();
    for row in 0..request.rows {
        for column in 0..request.columns {
            let tile = row * request.columns + column;
            if column + 1 < request.columns {
                let next = tile + 1;
                let offset = mosaic::estimate(&planes[tile], &planes[next], Side::Right).map_err(
                    |error| {
                        Error::new(format!(
                            "the tiles {} and {} do not register: {error}",
                            tile + 1,
                            next + 1
                        ))
                    },
                )?;
                edges.push(Edge::new(tile, next, offset));
            }
            if row + 1 < request.rows {
                let next = tile + request.columns;
                let offset = mosaic::estimate(&planes[tile], &planes[next], Side::Below).map_err(
                    |error| {
                        Error::new(format!(
                            "the tiles {} and {} do not register: {error}",
                            tile + 1,
                            next + 1
                        ))
                    },
                )?;
                edges.push(Edge::new(tile, next, offset));
            }
        }
    }
    let solved = mosaic::solve(images.len(), &edges)?;
    Ok(solved
        .iter()
        .map(|offset| {
            Offset::new(
                (offset.x as f64 * scale[0]).round() as i64,
                (offset.y as f64 * scale[1]).round() as i64,
            )
        })
        .collect())
}

fn flat(plane: &Plane) -> bool {
    let low = plane.values().iter().copied().fold(f32::INFINITY, f32::min);
    let high = plane
        .values()
        .iter()
        .copied()
        .fold(f32::NEG_INFINITY, f32::max);
    !(high.is_finite() && low.is_finite() && high > low)
}

fn propagate(
    solved: Vec<Option<Vec<Offset>>>,
    failure: Option<Error>,
) -> Result<Vec<Vec<Offset>>, Error> {
    let Some(seed) = solved.iter().position(Option::is_some) else {
        return Err(failure.unwrap_or_else(|| Error::new("the tiles do not register")));
    };
    let mut last = solved[seed]
        .clone()
        .expect("the seed frame carries offsets");
    let mut positions = solved;
    for slot in positions.iter_mut().take(seed) {
        *slot = Some(last.clone());
    }
    for slot in positions.iter_mut().skip(seed) {
        match slot {
            Some(held) => last = held.clone(),
            None => *slot = Some(last.clone()),
        }
    }
    Ok(positions
        .into_iter()
        .map(|slot| slot.expect("every frame carries offsets"))
        .collect())
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
