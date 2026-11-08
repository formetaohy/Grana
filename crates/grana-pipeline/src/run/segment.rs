use std::ops::Range;
use std::sync::atomic::{AtomicBool, Ordering};

use grana_analysis::{LabelBuilder, LabelSlab};
use grana_object::{Mask, Roi, SurfaceSet};
use grana_scene::{Drift, ObjectFrame, SegmentParams};
use grana_segment::{Segmenter, Settings, assemble};
use grana_volume::{Level, Region};

use crate::channels::Channels;
use crate::event::Event;

mod level;

use level::Leveling;

const LABEL_SLAB: u32 = 32;

pub struct SegmentRequest {
    channel: usize,
    params: SegmentParams,
    region: Region,
    drift: Drift,
    roi: Option<Roi>,
}

impl SegmentRequest {
    pub fn new(
        channel: usize,
        params: SegmentParams,
        region: Region,
        drift: Drift,
        roi: Option<Roi>,
    ) -> Self {
        Self {
            channel,
            params,
            region,
            drift,
            roi,
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
    let region = request
        .region
        .voxel_range(info)
        .ok_or_else(|| "the segmentation region overlaps no voxel".to_string())?;
    let voxels = (region[0].end - region[0].start) as u64
        * (region[1].end - region[1].start) as u64
        * (region[2].end - region[2].start) as u64;
    if voxels.saturating_mul(8) > 2 << 30 {
        return Err(
            "the segmentation region holds more than 2 GiB of slice labels; narrow the region"
                .to_string(),
        );
    }
    let depth = region[2].end - region[2].start;
    let settings = Settings::new(
        request.params.cellprob_threshold(),
        request.params.flow_threshold(),
        request.params.minimum_size(),
        request.params.tile_overlap(),
        request.params.slice_overlap(),
    );
    let version = match request.params.model() {
        grana_scene::SegmentModel::Cpsam => grana_weight::ModelVersion::Cpsam,
        grana_scene::SegmentModel::CpsamV2 => grana_weight::ModelVersion::CpsamV2,
    };
    let path = match grana_weight::weights(version, cancel, &mut |done, total| {
        sink(Event::Progress {
            time: 0,
            done: done as usize,
            total: total as usize,
        });
    }) {
        Ok(path) => path,
        Err(grana_weight::Error::Cancelled) => return Ok(()),
        Err(error) => return Err(error.to_string()),
    };
    let segmenter = Segmenter::open(&path, settings).map_err(|error| error.to_string())?;
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
        let mask = match request.roi.as_ref() {
            Some(roi) => Some(roi.mask(info, time)?),
            None => None,
        };
        let Some(ranges) = clip(&region, mask.as_ref(), info) else {
            continue;
        };
        let width = ranges[0].end - ranges[0].start;
        let height = ranges[1].end - ranges[1].start;
        let leveling = Leveling::of(reader, time, request.channel, &ranges, mask.as_ref(), shift)?;
        let mut slices = Vec::with_capacity(depth as usize);
        for (offset, z) in ranges[2].clone().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                return Ok(());
            }
            let slice = [ranges[0].clone(), ranges[1].clone(), z..z + 1];
            let values = reader
                .read_region(time, 0, request.channel, &slice, shift)
                .map_err(|error| error.to_string())?;
            let mut plane = leveling.apply(&values);
            let kept = match mask.as_ref() {
                Some(mask) => keep_plane(&mut plane, mask, &ranges, z),
                None => true,
            };
            let mut labels = if kept {
                segmenter
                    .segment(&plane, width, height)
                    .map_err(|error| format!("the segmentation model failed: {error}"))?
            } else {
                vec![0u32; width as usize * height as usize]
            };
            if let Some(mask) = mask.as_ref() {
                keep_labels(&mut labels, mask, &ranges, z);
            }
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

fn clip(region: &[Range<u32>; 3], mask: Option<&Mask>, level: Level) -> Option<[Range<u32>; 3]> {
    let extent = level.extent();
    let extent = [extent.x(), extent.y(), extent.z()];
    let mut ranges = region.clone();
    if let Some(mask) = mask {
        for (axis, bounds) in mask.bounds().into_iter().enumerate() {
            let limit = i64::from(extent[axis]);
            let low = bounds.start.clamp(0, limit) as u32;
            let high = bounds.end.clamp(0, limit) as u32;
            ranges[axis] = ranges[axis].start.max(low)..ranges[axis].end.min(high);
        }
    }
    ranges
        .iter()
        .all(|range| range.start < range.end)
        .then_some(ranges)
}

fn keep_plane(plane: &mut [f32], mask: &Mask, ranges: &[Range<u32>; 3], z: u32) -> bool {
    let width = (ranges[0].end - ranges[0].start) as usize;
    let height = (ranges[1].end - ranges[1].start) as usize;
    let mut any = false;
    for y in 0..height {
        let row = y * width;
        for x in 0..width {
            if mask.contains_cell([ranges[0].start + x as u32, ranges[1].start + y as u32, z]) {
                any = true;
            } else {
                plane[row + x] = 0.0;
            }
        }
    }
    any
}

fn keep_labels(labels: &mut [u32], mask: &Mask, ranges: &[Range<u32>; 3], z: u32) {
    let width = (ranges[0].end - ranges[0].start) as usize;
    let height = (ranges[1].end - ranges[1].start) as usize;
    for y in 0..height {
        let row = y * width;
        for x in 0..width {
            if !mask.contains_cell([ranges[0].start + x as u32, ranges[1].start + y as u32, z]) {
                labels[row + x] = 0;
            }
        }
    }
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
