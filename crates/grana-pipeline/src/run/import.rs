use std::sync::atomic::{AtomicBool, Ordering};

use crate::channels::Channels;
use grana_analysis::{LabelBuilder, LabelSlab};
use grana_scene::{Drifts, ObjectFrame};
use grana_volume::{Pyramid, VoxelData};

use crate::event::Event;

const TILE_X: u32 = 128;
const TILE_Y: u32 = 128;
const TILE_Z: u32 = 32;

pub struct ImportRequest {
    channel: usize,
    label: String,
    drifts: Drifts,
}

impl ImportRequest {
    pub fn new(channel: usize, label: &str, drifts: Drifts) -> Self {
        assert!(!label.trim().is_empty(), "an import names a label image");
        Self {
            channel,
            label: label.to_string(),
            drifts,
        }
    }

    pub fn label(&self) -> &str {
        &self.label
    }
}

pub fn import(
    reader: &Channels,
    request: ImportRequest,
    cancel: &AtomicBool,
    sink: &mut dyn FnMut(Event),
) -> Result<(), String> {
    let labels = reader.labels().map_err(String::from)?;
    let label = labels
        .get(&request.label)
        .ok_or_else(|| format!("the dataset holds no label image named {}", request.label))?;
    let pyramid = reader.pyramid();
    let (label_level, image_level) = match_levels(pyramid, label).ok_or_else(|| {
        format!(
            "the label image {} holds no resolution level of the dataset",
            request.label
        )
    })?;
    let info = label
        .level(label_level)
        .expect("a matched label level exists");
    let extent = info.extent();
    let limits = [extent.x(), extent.y(), extent.z()];
    let frames = pyramid.frame_count();
    let mut cores = Vec::new();
    for z in (0..limits[2]).step_by(TILE_Z as usize) {
        for y in (0..limits[1]).step_by(TILE_Y as usize) {
            for x in (0..limits[0]).step_by(TILE_X as usize) {
                cores.push((
                    [x, y, z],
                    [
                        TILE_X.min(limits[0] - x),
                        TILE_Y.min(limits[1] - y),
                        TILE_Z.min(limits[2] - z),
                    ],
                ));
            }
        }
    }
    let total = cores.len() * frames as usize;
    let mut done = 0usize;
    sink(Event::Progress {
        time: 0,
        done,
        total,
    });
    for time in 0..frames {
        let warp = request.drifts.warp(request.channel, time);
        let voxels = info
            .basis()
            .untransform(warp.translation())
            .map(|value| value.round() as i64);
        let mut builder = LabelBuilder::new(image_level, info, pyramid.channel_count());
        for (origin, core) in &cores {
            if cancel.load(Ordering::Relaxed) {
                return Ok(());
            }
            let mut span = [0u32; 3];
            for axis in 0..3 {
                span[axis] = (core[axis] + 1).min(limits[axis] - origin[axis]);
            }
            let ranges = [
                origin[0]..origin[0] + span[0],
                origin[1]..origin[1] + span[1],
                origin[2]..origin[2] + span[2],
            ];
            let data = match warp.field_grid() {
                None => label
                    .read(time, label_level, &ranges, voxels)
                    .map_err(|error| error.to_string())?,
                Some(_) => warped(label, time, label_level, &ranges, info, warp, limits)?,
            };
            let mut channels = Vec::with_capacity(pyramid.channel_count());
            for channel in 0..pyramid.channel_count() {
                let values: VoxelData = reader
                    .read_region(
                        time,
                        image_level,
                        channel,
                        &ranges,
                        request.drifts.warp(channel, time),
                    )
                    .map_err(|error| error.to_string())?;
                channels.push(values);
            }
            builder.push(LabelSlab {
                origin: *origin,
                core: *core,
                extent: span,
                labels: &data,
                channels: &channels,
            });
            done += 1;
            sink(Event::Progress { time, done, total });
        }
        if let Some(set) = builder.finish() {
            sink(Event::Frame {
                time,
                frame: ObjectFrame::Surfaces(set),
            });
        }
    }
    if cancel.load(Ordering::Relaxed) {
        return Ok(());
    }
    sink(Event::Done);
    Ok(())
}

fn warped(
    label: &grana_image::Label,
    time: u32,
    level: usize,
    ranges: &[std::ops::Range<u32>; 3],
    info: grana_volume::Level,
    warp: &grana_warp::Warp,
    limits: [u32; 3],
) -> Result<Vec<u32>, String> {
    use glam::Vec3;
    let inverse = info.basis().inverse();
    let magnitude = warp.magnitude();
    let pad = [0, 1, 2].map(|axis| {
        let row = inverse.row(axis);
        (magnitude * (row.x.abs() + row.y.abs() + row.z.abs())).ceil() as i64 + 1
    });
    let source = [0, 1, 2].map(|axis| {
        let start = (i64::from(ranges[axis].start) - pad[axis]).clamp(0, i64::from(limits[axis]));
        let end = (i64::from(ranges[axis].end) + pad[axis]).clamp(0, i64::from(limits[axis]));
        start as u32..(end.max(start) as u32)
    });
    if source.iter().any(|range| range.start >= range.end) {
        return Ok(vec![
            0;
            ranges
                .iter()
                .map(|range| (range.end - range.start) as usize)
                .product()
        ]);
    }
    let raw = label
        .read(time, level, &source, [0; 3])
        .map_err(|error| error.to_string())?;
    let dims = [0, 1, 2].map(|axis| (source[axis].end - source[axis].start) as usize);
    let origin = Vec3::from(info.origin());
    let matrix = info.basis().matrix();
    let width = (ranges[0].end - ranges[0].start) as usize;
    let height = (ranges[1].end - ranges[1].start) as usize;
    let depth = (ranges[2].end - ranges[2].start) as usize;
    let mut values = vec![0u32; width * height * depth];
    for z in 0..depth {
        for y in 0..height {
            for x in 0..width {
                let index = Vec3::new(
                    ranges[0].start as f32 + x as f32,
                    ranges[1].start as f32 + y as f32,
                    ranges[2].start as f32 + z as f32,
                );
                let centre = origin + matrix * (index + Vec3::splat(0.5));
                let offset = inverse * Vec3::from(warp.sample(centre.to_array()));
                let point = index + offset;
                let nearest = [
                    point.x.round() as i64,
                    point.y.round() as i64,
                    point.z.round() as i64,
                ];
                if (0..3).any(|axis| nearest[axis] < 0 || nearest[axis] >= i64::from(limits[axis]))
                {
                    continue;
                }
                let local = [
                    nearest[0] as usize - source[0].start as usize,
                    nearest[1] as usize - source[1].start as usize,
                    nearest[2] as usize - source[2].start as usize,
                ];
                if (0..3).any(|axis| local[axis] >= dims[axis]) {
                    continue;
                }
                values[x + width * (y + height * z)] =
                    raw[local[0] + dims[0] * (local[1] + dims[1] * local[2])];
            }
        }
    }
    Ok(values)
}

fn match_levels(pyramid: &Pyramid, label: &grana_image::Label) -> Option<(usize, usize)> {
    for label_level in 0..label.level_count() {
        let info = label.level(label_level)?;
        if let Some(image_level) = pyramid.levels().iter().position(|level| {
            level.extent() == info.extent()
                && level.basis().close(info.basis())
                && close_origin(level.origin(), info.origin())
        }) {
            return Some((label_level, image_level));
        }
    }
    None
}

fn close_origin(left: [f32; 3], right: [f32; 3]) -> bool {
    left.into_iter()
        .zip(right)
        .all(|(left, right)| (left - right).abs() <= 1.0e-4 * left.abs().max(right.abs()).max(1.0))
}
