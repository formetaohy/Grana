use std::ops::Range;
use std::sync::atomic::{AtomicBool, Ordering};

use grana_object::{Mask, Roi};
use grana_scene::{Drifts, ObjectFrame, SegmentParams};
use grana_segment::{Segmenter, Settings, assemble};
use grana_volume::{Level, Region};

use crate::channels::Channels;
use crate::event::Event;
use crate::run::domain::Domain;
use crate::run::labelled;
use crate::run::level::Leveling;

mod prompt;

pub struct SegmentRequest {
    channel: usize,
    params: SegmentParams,
    region: Region,
    drifts: Drifts,
    roi: Option<Roi>,
    seeds: Vec<Vec<[f32; 3]>>,
}

impl SegmentRequest {
    pub fn new(
        channel: usize,
        params: SegmentParams,
        region: Region,
        drifts: Drifts,
        roi: Option<Roi>,
    ) -> Self {
        Self {
            channel,
            params,
            region,
            drifts,
            roi,
            seeds: Vec::new(),
        }
    }

    pub fn with_seeds(mut self, seeds: Vec<Vec<[f32; 3]>>) -> Self {
        self.seeds = seeds;
        self
    }
}

pub(super) fn segmenter(
    params: &SegmentParams,
    cancel: &AtomicBool,
    sink: &mut dyn FnMut(Event),
) -> Result<Option<(Segmenter, Settings)>, String> {
    let settings = Settings::new(
        params.cellprob_threshold(),
        params.flow_threshold(),
        params.minimum_size(),
        params.tile_overlap(),
        params.slice_overlap(),
    );
    let path = match grana_weight::weights(params.model(), cancel, &mut |done, total| {
        sink(Event::Progress {
            time: 0,
            done: done as usize,
            total: total as usize,
        });
    }) {
        Ok(path) => path,
        Err(grana_weight::Error::Cancelled) => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    let segmenter = Segmenter::open(&path, settings).map_err(|error| error.to_string())?;
    Ok(Some((segmenter, settings)))
}

pub fn segment(
    reader: &Channels,
    request: SegmentRequest,
    cancel: &AtomicBool,
    sink: &mut dyn FnMut(Event),
) -> Result<(), String> {
    if request.seeds.iter().any(|frame| !frame.is_empty()) {
        return prompt::segment(reader, request, cancel, sink);
    }
    let info = reader.pyramid().level(0);
    let bounds = request
        .region
        .voxel_range(info)
        .ok_or_else(|| "the segmentation region overlaps no voxel".to_string())?;
    let voxels = (bounds[0].end - bounds[0].start) as u64
        * (bounds[1].end - bounds[1].start) as u64
        * (bounds[2].end - bounds[2].start) as u64;
    if voxels.saturating_mul(8) > 2 << 30 {
        return Err(
            "the segmentation region holds more than 2 GiB of slice labels; narrow the region"
                .to_string(),
        );
    }
    let depth = bounds[2].end - bounds[2].start;
    let Some((segmenter, _)) = segmenter(&request.params, cancel, sink)? else {
        return Ok(());
    };
    let domain = Domain::new(request.region, info);
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
        let warp = request.drifts.warp(request.channel, time);
        let mask = domain.mask(request.roi.as_ref(), time)?;
        let Some(ranges) = clip(&bounds, mask.as_ref(), info) else {
            continue;
        };
        let width = ranges[0].end - ranges[0].start;
        let height = ranges[1].end - ranges[1].start;
        let leveling = Leveling::at(
            reader,
            time,
            request.channel,
            0,
            &ranges,
            mask.as_ref(),
            warp,
        )?;
        let mut slices = Vec::with_capacity(depth as usize);
        for (offset, z) in ranges[2].clone().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                return Ok(());
            }
            let slice = [ranges[0].clone(), ranges[1].clone(), z..z + 1];
            let values = reader
                .read_region(time, 0, request.channel, &slice, warp)
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
        let volume = assemble(&mut slices, width, height, request.params.slice_overlap())
            .map_err(|error| format!("the segmentation slices do not assemble: {error}"))?;
        if volume.count() == 0 {
            continue;
        }
        if let Some(set) =
            labelled::surfaces(reader, time, 0, info, &ranges, volume.labels(), warp)?
        {
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
