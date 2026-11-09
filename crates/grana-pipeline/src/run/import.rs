use std::sync::atomic::{AtomicBool, Ordering};

use crate::channels::Channels;
use grana_analysis::{LabelBuilder, LabelSlab};
use grana_scene::{Drift, ObjectFrame};
use grana_volume::{Pyramid, VoxelData};

use crate::event::Event;

const TILE_X: u32 = 128;
const TILE_Y: u32 = 128;
const TILE_Z: u32 = 32;

pub struct ImportRequest {
    label: String,
    drift: Drift,
}

impl ImportRequest {
    pub fn new(label: &str, drift: Drift) -> Self {
        assert!(!label.trim().is_empty(), "an import names a label image");
        Self {
            label: label.to_string(),
            drift,
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
    let ratio = (1u64 << image_level) as f32;
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
        let shift = request.drift.offset(time);
        let voxels = shift.map(|value| (value / ratio).round() as i64);
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
            let data = label
                .read(time, label_level, &ranges, voxels)
                .map_err(|error| error.to_string())?;
            let mut channels = Vec::with_capacity(pyramid.channel_count());
            for channel in 0..pyramid.channel_count() {
                let values: VoxelData = reader
                    .read_region(time, image_level, channel, &ranges, shift)
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
