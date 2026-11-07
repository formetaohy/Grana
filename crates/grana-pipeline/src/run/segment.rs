use std::ops::Range;
use std::sync::atomic::{AtomicBool, Ordering};

use grana_analysis::{LabelBuilder, LabelSlab};
use grana_object::SurfaceSet;
use grana_scene::{Drift, ObjectFrame, SegmentParams};
use grana_segment::{Segmenter, Settings, assemble};
use grana_volume::{Intensity, Region, VoxelData};

use crate::channels::Channels;
use crate::event::Event;

const LABEL_SLAB: u32 = 32;
const PERCENTILE_SAMPLES: usize = 1 << 16;

pub struct SegmentRequest {
    channel: usize,
    params: SegmentParams,
    region: Region,
    drift: Drift,
}

impl SegmentRequest {
    pub fn new(channel: usize, params: SegmentParams, region: Region, drift: Drift) -> Self {
        Self {
            channel,
            params,
            region,
            drift,
        }
    }
}

pub fn segment(
    reader: &Channels,
    request: SegmentRequest,
    cancel: &AtomicBool,
    sink: &mut dyn FnMut(Event),
) -> Result<(), String> {
    let info = reader.pyramid().level(0);
    let ranges = request
        .region
        .voxel_range(info)
        .ok_or_else(|| "the segmentation region overlaps no voxel".to_string())?;
    let width = ranges[0].end - ranges[0].start;
    let height = ranges[1].end - ranges[1].start;
    let depth = ranges[2].end - ranges[2].start;
    let voxels = u64::from(width) * u64::from(height) * u64::from(depth);
    if voxels.saturating_mul(8) > 2 << 30 {
        return Err(
            "the segmentation region holds more than 2 GiB of slice labels; narrow the region"
                .to_string(),
        );
    }
    let settings = Settings::new(
        request.params.cellprob_threshold(),
        request.params.flow_threshold(),
        request.params.minimum_size(),
        request.params.tile_overlap(),
        request.params.slice_overlap(),
    );
    let mut segmenter = Segmenter::open(request.params.model(), settings)
        .map_err(|error| format!("the segmentation model cannot open: {error}"))?;
    let frames = reader.pyramid().frame_count();
    let total = frames as usize * depth as usize;
    sink(Event::Progress {
        time: 0,
        done: 0,
        total,
    });
    for time in 0..frames {
        if cancel.load(Ordering::Relaxed) {
            return Ok(());
        }
        let shift = request.drift.offset(time);
        let mut slices = Vec::with_capacity(depth as usize);
        for (offset, z) in ranges[2].clone().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                return Ok(());
            }
            let slice = [ranges[0].clone(), ranges[1].clone(), z..z + 1];
            let values = reader
                .read_region(time, 0, request.channel, &slice, shift)
                .map_err(|error| error.to_string())?;
            let plane = normalized(
                &values,
                reader.pyramid().channels()[request.channel].intensity(),
            );
            let labels = segmenter
                .segment(&plane, width, height)
                .map_err(|error| format!("the segmentation model failed: {error}"))?;
            slices.push(labels);
            let done = time as usize * depth as usize + offset + 1;
            sink(Event::Progress { time, done, total });
        }
        let volume = assemble(&slices, width, height, request.params.slice_overlap())
            .map_err(|error| format!("the segmentation slices do not assemble: {error}"))?;
        if volume.count() == 0 {
            continue;
        }
        if let Some(set) = surfaces(reader, time, info, &ranges, &volume, shift)? {
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

fn surfaces(
    reader: &Channels,
    time: u32,
    info: grana_volume::Level,
    ranges: &[Range<u32>; 3],
    volume: &grana_segment::Volume,
    shift: [f32; 3],
) -> Result<Option<SurfaceSet>, String> {
    let width = ranges[0].end - ranges[0].start;
    let height = ranges[1].end - ranges[1].start;
    let depth = ranges[2].end - ranges[2].start;
    let plane = width as usize * height as usize;
    let mut builder = LabelBuilder::new(0, info, reader.pyramid().channel_count());
    let mut z = 0u32;
    while z < depth {
        let core = LABEL_SLAB.min(depth - z);
        let span = (core + 1).min(depth - z);
        let start = z as usize * plane;
        let labels = &volume.labels()[start..start + span as usize * plane];
        let origin = [ranges[0].start, ranges[1].start, ranges[2].start + z];
        let extent = [
            ranges[0].clone(),
            ranges[1].clone(),
            origin[2]..origin[2] + span,
        ];
        let mut channels = Vec::with_capacity(reader.pyramid().channel_count());
        for channel in 0..reader.pyramid().channel_count() {
            channels.push(
                reader
                    .read_region(time, 0, channel, &extent, shift)
                    .map_err(|error| error.to_string())?,
            );
        }
        builder.push(LabelSlab {
            origin,
            core: [width, height, core],
            extent: [width, height, span],
            labels,
            channels: &channels,
        });
        z += core;
    }
    Ok(builder.finish())
}

fn normalized(values: &VoxelData, intensity: Intensity) -> Vec<f32> {
    let count = values.voxel_count();
    let mut plane = vec![0.0f32; count];
    values.write_values(&mut plane);
    let (low, high) = match intensity.span() {
        span if span > 0.0 => (intensity.min(), intensity.max()),
        _ => {
            let mut low = f32::INFINITY;
            let mut high = f32::NEG_INFINITY;
            for value in &plane {
                if value.is_finite() {
                    low = low.min(*value);
                    high = high.max(*value);
                }
            }
            (low, high)
        }
    };
    if !(low.is_finite() && high.is_finite() && high > low) {
        plane.fill(0.0);
        return plane;
    }
    let span = high - low;
    for value in &mut plane {
        *value = ((*value - low) / span).clamp(0.0, 1.0);
    }
    let (first, last) = percentiles(&plane);
    if last <= first {
        plane.fill(0.0);
        return plane;
    }
    let span = last - first;
    for value in &mut plane {
        *value = ((*value - first) / span).clamp(0.0, 1.0);
    }
    plane
}

fn percentiles(plane: &[f32]) -> (f32, f32) {
    let stride = (plane.len() / PERCENTILE_SAMPLES).max(1);
    let mut samples = plane
        .iter()
        .step_by(stride)
        .copied()
        .filter(|value| value.is_finite())
        .collect::<Vec<_>>();
    if samples.is_empty() {
        return (0.0, 0.0);
    }
    samples.sort_unstable_by(f32::total_cmp);
    let last = samples.len() - 1;
    (samples[last / 100], samples[last - last / 100])
}
