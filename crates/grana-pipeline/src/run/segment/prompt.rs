use std::sync::atomic::{AtomicBool, Ordering};

use grana_analysis::{LabelBuilder, LabelSlab};
use grana_object::{Mask, SurfaceSet};
use grana_scene::ObjectFrame;
use grana_segment::{Segmenter, Settings};
use grana_volume::{Extent, Level};

use crate::channels::Channels;
use crate::event::Event;
use crate::run::domain::Domain;

use super::SegmentRequest;
use super::segmenter;

const WINDOW_LIMIT: u32 = 1024;
const WINDOW_GROWTH: u32 = 3;
const BLOCK_LIMIT: u64 = 96 << 20;

pub(super) fn segment(
    reader: &Channels,
    request: SegmentRequest,
    cancel: &AtomicBool,
    sink: &mut dyn FnMut(Event),
) -> Result<(), String> {
    let pyramid = reader.pyramid();
    let info = pyramid.level(0);
    let frames = pyramid.frame_count();
    if request.seeds.len() != frames as usize {
        return Err("the seed points cover every frame of the dataset".to_string());
    }
    let Some((segmenter, settings)) = segmenter(&request.params, cancel, sink)? else {
        return Ok(());
    };
    let domain = Domain::new(request.region, info);
    let mut found = 0usize;
    for span in spans(&request.seeds, frames) {
        let mut prompts = request.seeds[span.start as usize].clone();
        for time in order(span) {
            if cancel.load(Ordering::Relaxed) {
                return Ok(());
            }
            if prompts.is_empty() {
                break;
            }
            let mask = domain.mask(request.roi.as_ref(), time)?;
            let grower = Grow {
                reader,
                segmenter: &segmenter,
                info,
                channel: request.channel,
                roi: mask.as_ref(),
                warp: request.drifts.warp(request.channel, time),
                time,
                settings,
            };
            let total = prompts.len();
            sink(Event::Progress {
                time,
                done: 0,
                total,
            });
            let mut blocks: Vec<Block> = Vec::with_capacity(total);
            for (index, prompt) in prompts.iter().enumerate() {
                if cancel.load(Ordering::Relaxed) {
                    return Ok(());
                }
                let duplicate = blocks.iter().any(|block| block.contains(*prompt, info));
                if !duplicate && let Some(block) = grower.object(*prompt, cancel)? {
                    blocks.push(block);
                }
                sink(Event::Progress {
                    time,
                    done: index + 1,
                    total,
                });
            }
            if blocks.is_empty() {
                break;
            }
            found += blocks.len();
            if let Some(surfaces) = surfaces(reader, time, info, &blocks, &request.drifts)? {
                sink(Event::Frame {
                    time,
                    frame: ObjectFrame::Surfaces(surfaces),
                });
            }
            prompts = blocks.iter().map(|block| block.centroid).collect();
        }
    }
    if cancel.load(Ordering::Relaxed) {
        return Ok(());
    }
    if found == 0 {
        return Err("the seed points mark no object".to_string());
    }
    sink(Event::Done);
    Ok(())
}

struct Span {
    start: u32,
    end: u32,
    forward: bool,
}

fn spans(seeds: &[Vec<[f32; 3]>], frames: u32) -> Vec<Span> {
    let seeded = (0..frames)
        .filter(|time| !seeds[*time as usize].is_empty())
        .collect::<Vec<_>>();
    let mut spans = Vec::new();
    if let Some(first) = seeded.first().copied()
        && first > 0
    {
        spans.push(Span {
            start: first,
            end: 0,
            forward: false,
        });
    }
    for (index, &start) in seeded.iter().enumerate() {
        spans.push(Span {
            start,
            end: seeded.get(index + 1).copied().unwrap_or(frames),
            forward: true,
        });
    }
    spans
}

fn order(span: Span) -> Vec<u32> {
    let mut order = vec![span.start];
    if span.forward {
        order.extend(span.start + 1..span.end);
    } else {
        let mut time = span.start;
        while time > 0 {
            time -= 1;
            order.push(time);
        }
    }
    order
}

struct Grow<'a> {
    reader: &'a Channels,
    segmenter: &'a Segmenter,
    info: Level,
    channel: usize,
    roi: Option<&'a Mask>,
    warp: &'a grana_warp::Warp,
    time: u32,
    settings: Settings,
}

impl Grow<'_> {
    fn object(&self, prompt: [f32; 3], cancel: &AtomicBool) -> Result<Option<Block>, String> {
        let info = self.info;
        let extent = info.extent();
        let limits = [extent.x(), extent.y(), extent.z()];
        let voxel = info.voxel_of(prompt);
        if voxel.iter().any(|value| !value.is_finite()) {
            return Ok(None);
        }
        let seed = [0, 1, 2]
            .map(|axis| (voxel[axis].floor() as i64).clamp(0, limits[axis] as i64 - 1) as u32);
        if let Some(roi) = self.roi
            && !roi.contains_cell(seed)
        {
            return Ok(None);
        }
        let mut slices: Vec<(u32, Mask2d)> = Vec::new();
        let mut center = [seed[0] as f32 + 0.5, seed[1] as f32 + 0.5];
        let mut previous: Option<usize> = None;
        let mut seed_slice: Option<usize> = None;
        for z in seed[2]..limits[2] {
            if cancel.load(Ordering::Relaxed) {
                return Ok(None);
            }
            let Some(mask) = self.slice(z, center)? else {
                break;
            };
            if let Some(previous) = previous.map(|index| &slices[index].1)
                && !links(previous, &mask, self.settings.slice_overlap())
            {
                break;
            }
            center = mask.centroid();
            slices.push((z, mask));
            previous = Some(slices.len() - 1);
            if z == seed[2] {
                seed_slice = previous;
            }
        }
        let Some(above) = seed_slice else {
            return Ok(None);
        };
        let mut z = seed[2];
        let mut below = above;
        center = slices[above].1.centroid();
        while z > 0 {
            z -= 1;
            if cancel.load(Ordering::Relaxed) {
                return Ok(None);
            }
            let Some(mask) = self.slice(z, center)? else {
                break;
            };
            if !links(&slices[below].1, &mask, self.settings.slice_overlap()) {
                break;
            }
            center = mask.centroid();
            slices.push((z, mask));
            below = slices.len() - 1;
        }
        Block::of(&slices, info)
    }

    fn slice(&self, z: u32, center: [f32; 2]) -> Result<Option<Mask2d>, String> {
        let extent = self.info.extent();
        let mut side = self.segmenter.side().max(1);
        let mut center = center;
        let mut growth = 0;
        loop {
            let window = Window::around(center, side, extent);
            let ranges = [
                window.x..window.x + window.width,
                window.y..window.y + window.height,
                z..z + 1,
            ];
            let data = self
                .reader
                .read_region(self.time, 0, self.channel, &ranges, self.warp)
                .map_err(|error| error.to_string())?;
            let mut plane = vec![0.0f32; data.voxel_count()];
            data.write_values(&mut plane);
            if !self.covers(&window, z) {
                return Ok(None);
            }
            let seed = [
                (center[0] - window.x as f32 - 0.5).clamp(0.0, window.width as f32 - 1.0),
                (center[1] - window.y as f32 - 0.5).clamp(0.0, window.height as f32 - 1.0),
            ];
            let labels = self
                .segmenter
                .seeded(&plane, window.width, window.height, &[seed])
                .map_err(|error| format!("the segmentation model failed: {error}"))?;
            let bits = labels
                .into_iter()
                .enumerate()
                .map(|(index, label)| {
                    let x = window.x + (index as u32 % window.width);
                    let y = window.y + (index as u32 / window.width);
                    label > 0 && self.kept(x, y, z)
                })
                .collect();
            let mask = Mask2d { window, bits };
            if mask.count() == 0 {
                return Ok(None);
            }
            if !mask.touches_border()
                || mask.window.covers(extent)
                || growth >= WINDOW_GROWTH
                || mask.window.side() >= WINDOW_LIMIT
            {
                return Ok(Some(mask));
            }
            center = mask.centroid();
            side = (side * 2).min(WINDOW_LIMIT);
            growth += 1;
        }
    }

    fn covers(&self, window: &Window, z: u32) -> bool {
        let Some(roi) = self.roi else {
            return true;
        };
        (0..window.height)
            .any(|y| (0..window.width).any(|x| roi.contains_cell([window.x + x, window.y + y, z])))
    }

    fn kept(&self, x: u32, y: u32, z: u32) -> bool {
        self.roi.is_none_or(|roi| roi.contains_cell([x, y, z]))
    }
}

#[derive(Clone)]
struct Window {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

impl Window {
    fn around(center: [f32; 2], side: u32, extent: Extent) -> Self {
        let width = side.min(extent.x()).max(1);
        let height = side.min(extent.y()).max(1);
        let x = (center[0].floor() as i64 - i64::from(width) / 2)
            .clamp(0, i64::from(extent.x()) - i64::from(width)) as u32;
        let y = (center[1].floor() as i64 - i64::from(height) / 2)
            .clamp(0, i64::from(extent.y()) - i64::from(height)) as u32;
        Self {
            x,
            y,
            width,
            height,
        }
    }

    fn side(&self) -> u32 {
        self.width.max(self.height)
    }

    fn covers(&self, extent: Extent) -> bool {
        self.width == extent.x() && self.height == extent.y()
    }
}

struct Mask2d {
    window: Window,
    bits: Vec<bool>,
}

impl Mask2d {
    fn count(&self) -> usize {
        self.bits.iter().filter(|bit| **bit).count()
    }

    fn centroid(&self) -> [f32; 2] {
        let mut sum = [0.0f64; 2];
        let mut count = 0u64;
        for (index, bit) in self.bits.iter().enumerate() {
            if !*bit {
                continue;
            }
            sum[0] += f64::from(self.window.x) + (index % self.window.width as usize) as f64 + 0.5;
            sum[1] += f64::from(self.window.y) + (index / self.window.width as usize) as f64 + 0.5;
            count += 1;
        }
        let count = count.max(1) as f64;
        [(sum[0] / count) as f32, (sum[1] / count) as f32]
    }

    fn touches_border(&self) -> bool {
        let width = self.window.width as usize;
        let height = self.window.height as usize;
        let first_row = self.bits[..width].iter().any(|bit| *bit);
        let last_row = self.bits[(height - 1) * width..].iter().any(|bit| *bit);
        let first_column = (0..height).any(|y| self.bits[y * width]);
        let last_column = (0..height).any(|y| self.bits[y * width + width - 1]);
        first_row || last_row || first_column || last_column
    }

    fn contains(&self, x: i64, y: i64) -> bool {
        let local_x = x - i64::from(self.window.x);
        let local_y = y - i64::from(self.window.y);
        if local_x < 0
            || local_y < 0
            || local_x >= i64::from(self.window.width)
            || local_y >= i64::from(self.window.height)
        {
            return false;
        }
        self.bits[(local_y as usize) * self.window.width as usize + local_x as usize]
    }
}

fn links(previous: &Mask2d, current: &Mask2d, required: f32) -> bool {
    let smaller = previous.count().min(current.count());
    if smaller == 0 {
        return false;
    }
    (shared(previous, current) as f64 / smaller as f64) >= f64::from(required)
}

fn shared(left: &Mask2d, right: &Mask2d) -> usize {
    let (small, large) = if left.bits.len() <= right.bits.len() {
        (left, right)
    } else {
        (right, left)
    };
    let width = small.window.width as usize;
    let mut count = 0;
    for (index, bit) in small.bits.iter().enumerate() {
        if !*bit {
            continue;
        }
        let x = i64::from(small.window.x) + (index % width) as i64;
        let y = i64::from(small.window.y) + (index / width) as i64;
        if large.contains(x, y) {
            count += 1;
        }
    }
    count
}

struct Block {
    origin: [u32; 3],
    extent: Extent,
    labels: Vec<u32>,
    centroid: [f32; 3],
}

impl Block {
    fn of(slices: &[(u32, Mask2d)], info: Level) -> Result<Option<Self>, String> {
        if slices.is_empty() {
            return Ok(None);
        }
        let mut low = [u32::MAX; 3];
        let mut high = [0u32; 3];
        for (z, mask) in slices {
            low[0] = low[0].min(mask.window.x);
            low[1] = low[1].min(mask.window.y);
            low[2] = low[2].min(*z);
            high[0] = high[0].max(mask.window.x + mask.window.width);
            high[1] = high[1].max(mask.window.y + mask.window.height);
            high[2] = high[2].max(*z + 1);
        }
        let extent = Extent::new(high[0] - low[0], high[1] - low[1], high[2] - low[2]);
        if extent.voxel_count() > BLOCK_LIMIT {
            return Err(format!(
                "a seeded object covers more than {BLOCK_LIMIT} voxels; narrow the region"
            ));
        }
        let width = extent.x() as usize;
        let height = extent.y() as usize;
        let mut labels = vec![0u32; extent.voxel_count() as usize];
        let mut sum = [0.0f64; 3];
        let mut count = 0u64;
        for (z, mask) in slices {
            let depth = (*z - low[2]) as usize;
            for (index, bit) in mask.bits.iter().enumerate() {
                if !*bit {
                    continue;
                }
                let x = mask.window.x - low[0] + (index % mask.window.width as usize) as u32;
                let y = mask.window.y - low[1] + (index / mask.window.width as usize) as u32;
                labels[x as usize + width * (y as usize + height * depth)] = 1;
                sum[0] += f64::from(x);
                sum[1] += f64::from(y);
                sum[2] += depth as f64;
                count += 1;
            }
        }
        let count = count.max(1) as f64;
        let centroid = info.voxel_center([
            (sum[0] / count) as f32,
            (sum[1] / count) as f32,
            (sum[2] / count) as f32,
        ]);
        Ok(Some(Self {
            origin: low,
            extent,
            labels,
            centroid,
        }))
    }

    fn contains(&self, prompt: [f32; 3], info: Level) -> bool {
        let voxel = info.voxel_of(prompt);
        if voxel.iter().any(|value| !value.is_finite()) {
            return false;
        }
        let extent = self.extent;
        let local = [0, 1, 2].map(|axis| voxel[axis].floor() as i64 - i64::from(self.origin[axis]));
        let limits = [extent.x(), extent.y(), extent.z()];
        if (0..3).any(|axis| local[axis] < 0 || local[axis] >= i64::from(limits[axis])) {
            return false;
        }
        let index = local[0] as usize
            + extent.x() as usize * (local[1] as usize + extent.y() as usize * local[2] as usize);
        self.labels[index] != 0
    }
}

fn surfaces(
    reader: &Channels,
    time: u32,
    info: Level,
    blocks: &[Block],
    drifts: &grana_scene::Drifts,
) -> Result<Option<SurfaceSet>, String> {
    let channels = reader.pyramid().channel_count();
    let mut merged: Option<SurfaceSet> = None;
    for block in blocks {
        let extent = block.extent;
        let ranges = [
            block.origin[0]..block.origin[0] + extent.x(),
            block.origin[1]..block.origin[1] + extent.y(),
            block.origin[2]..block.origin[2] + extent.z(),
        ];
        let mut data = Vec::with_capacity(channels);
        for channel in 0..channels {
            data.push(
                reader
                    .read_region(time, 0, channel, &ranges, drifts.warp(channel, time))
                    .map_err(|error| error.to_string())?,
            );
        }
        let mut builder = LabelBuilder::new(0, info, channels);
        builder.push(LabelSlab {
            origin: block.origin,
            core: [extent.x(), extent.y(), extent.z()],
            extent: [extent.x(), extent.y(), extent.z()],
            labels: &block.labels,
            channels: &data,
        });
        let Some(surface) = builder.finish() else {
            continue;
        };
        merged = Some(match merged {
            Some(current) => current.merge(&surface).map_err(|error| error.to_string())?,
            None => surface,
        });
    }
    Ok(merged)
}
