use std::ops::Range;
use std::sync::atomic::{AtomicBool, Ordering};

use grana_object::{Mask, Roi, SurfaceSet};
use grana_scene::{Drifts, ObjectFrame, SegmentParams};
use grana_segment::{Segmenter, Settings, Volume, assemble};
use grana_volume::{Level, Region};

use crate::channels::Channels;
use crate::event::Event;
use crate::run::domain::Domain;
use crate::run::labelled;

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
    let Some((segmenter, _)) = segmenter(&request.params, cancel, sink)? else {
        return Ok(());
    };
    let slicer = Slicer {
        reader,
        request: &request,
        segmenter: &segmenter,
        info,
        domain: Domain::new(request.region, info),
        bounds,
    };
    if request.seeds.iter().any(|frame| !frame.is_empty()) {
        return prompt::segment(&slicer, cancel, sink);
    }
    dense(&slicer, reader.pyramid().frame_count(), cancel, sink)
}

fn dense(
    slicer: &Slicer<'_>,
    frames: u32,
    cancel: &AtomicBool,
    sink: &mut dyn FnMut(Event),
) -> Result<(), String> {
    let depth = slicer.depth() as usize;
    let total = frames as usize * depth;
    sink(Event::Progress {
        time: 0,
        done: 0,
        total,
    });
    for time in 0..frames {
        if cancel.load(Ordering::Relaxed) {
            return Ok(());
        }
        let base = time as usize * depth;
        let Some(assembly) = slicer.volume(time, cancel, &mut |done| {
            sink(Event::Progress {
                time,
                done: (base + done).min(total),
                total,
            });
        })?
        else {
            continue;
        };
        if assembly.volume.count() == 0 {
            continue;
        }
        if let Some(set) = slicer.surfaces(time, &assembly)? {
            sink(Event::Frame {
                time,
                frame: ObjectFrame::Surfaces(set),
            });
        }
    }
    if cancel.load(Ordering::Relaxed) {
        return Ok(());
    }
    sink(Event::Progress {
        time: frames.saturating_sub(1),
        done: total,
        total,
    });
    sink(Event::Done);
    Ok(())
}

pub(super) struct Slicer<'a> {
    pub(super) reader: &'a Channels,
    pub(super) request: &'a SegmentRequest,
    segmenter: &'a Segmenter,
    pub(super) info: Level,
    domain: Domain,
    bounds: [Range<u32>; 3],
}

pub(super) struct Assembly {
    pub(super) volume: Volume,
    pub(super) ranges: [Range<u32>; 3],
}

impl Slicer<'_> {
    pub(super) fn depth(&self) -> u32 {
        self.bounds[2].end - self.bounds[2].start
    }

    fn volume(
        &self,
        time: u32,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(usize),
    ) -> Result<Option<Assembly>, String> {
        let mask = self.domain.mask(self.request.roi.as_ref(), time)?;
        let Some(ranges) = clip(&self.bounds, mask.as_ref(), self.info) else {
            return Ok(None);
        };
        let warp = self.request.drifts.warp(self.request.channel, time);
        let no_data = self.reader.pyramid().channels()[self.request.channel].no_data();
        let width = ranges[0].end - ranges[0].start;
        let height = ranges[1].end - ranges[1].start;
        let mut slices = Vec::with_capacity((ranges[2].end - ranges[2].start) as usize);
        for (offset, z) in ranges[2].clone().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                return Ok(None);
            }
            let slice = [ranges[0].clone(), ranges[1].clone(), z..z + 1];
            let values = self
                .reader
                .read_region(time, 0, self.request.channel, &slice, warp)
                .map_err(|error| error.to_string())?;
            let mut plane = vec![0.0f32; values.voxel_count()];
            values.write_values_absent_as_nan(no_data, &mut plane);
            let mut absent = None;
            if no_data.is_some() {
                absent = Some(
                    plane
                        .iter()
                        .map(|value| !value.is_finite())
                        .collect::<Vec<_>>(),
                );
                for value in &mut plane {
                    if !value.is_finite() {
                        *value = 0.0;
                    }
                }
            }
            let kept = mask.as_ref().is_none_or(|mask| covers(mask, &ranges, z));
            let mut labels = if kept {
                self.segmenter
                    .segment(&plane, width, height)
                    .map_err(|error| format!("the segmentation model failed: {error}"))?
            } else {
                vec![0u32; width as usize * height as usize]
            };
            if let Some(absent) = absent {
                for (label, absent) in labels.iter_mut().zip(&absent) {
                    if *absent {
                        *label = 0;
                    }
                }
            }
            if let Some(mask) = mask.as_ref() {
                keep_labels(&mut labels, mask, &ranges, z);
            }
            slices.push(labels);
            progress(offset + 1);
        }
        let volume = assemble(
            &mut slices,
            width,
            height,
            self.request.params.slice_overlap(),
            self.request.params.minimum_size(),
        )
        .map_err(|error| format!("the segmentation slices do not assemble: {error}"))?;
        Ok(Some(Assembly { volume, ranges }))
    }

    pub(super) fn surfaces(
        &self,
        time: u32,
        assembly: &Assembly,
    ) -> Result<Option<SurfaceSet>, String> {
        labelled::surfaces(
            self.reader,
            time,
            0,
            self.info,
            &assembly.ranges,
            assembly.volume.labels(),
            &self.request.drifts,
        )
    }
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

fn covers(mask: &Mask, ranges: &[Range<u32>; 3], z: u32) -> bool {
    let width = (ranges[0].end - ranges[0].start) as usize;
    let height = (ranges[1].end - ranges[1].start) as usize;
    for y in 0..height {
        for x in 0..width {
            if mask.contains_cell([ranges[0].start + x as u32, ranges[1].start + y as u32, z]) {
                return true;
            }
        }
    }
    false
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
