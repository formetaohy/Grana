use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use grana_gpu::{Consumer, Ledger};
use grana_object::{Mask, ObjectIdSource, Roi, SurfaceSet};
use grana_scene::{Drifts, ObjectFrame, SegmentParams};
use grana_segment::{Segmenter, Settings, Volume};
use grana_volume::{Level, Region};

use crate::channels::Channels;
use crate::event::Event;
use crate::run::domain::Domain;
use crate::run::labelled;

mod prompt;

pub struct SegmentRequest {
    channel: usize,
    ids: ObjectIdSource,
    params: SegmentParams,
    region: Region,
    drifts: Drifts,
    roi: Option<Roi>,
    seeds: Vec<Vec<[f32; 3]>>,
}

impl SegmentRequest {
    pub fn new(
        channel: usize,
        ids: ObjectIdSource,
        params: SegmentParams,
        region: Region,
        drifts: Drifts,
        roi: Option<Roi>,
    ) -> Self {
        Self {
            channel,
            ids,
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
        params.foreground_threshold(),
        params.density_threshold(),
        params.minimum_size(),
    )
    .map_err(|error| error.to_string())?;
    let path = match grana_weight::weights(cancel, &mut |done, total| {
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
    let Some((segmenter, _)) = segmenter(&request.params, cancel, sink)? else {
        return Ok(());
    };
    let graphics = grana_gpu::Headless::shared()?;
    let ledger = Arc::clone(graphics.graphics().ledger());
    let slicer = Slicer {
        reader,
        request: &request,
        segmenter: &segmenter,
        ledger,
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
    ledger: Arc<Ledger>,
    pub(super) info: Level,
    domain: Domain,
    bounds: [Range<u32>; 3],
}

pub(super) struct Assembly {
    pub(super) volume: Volume,
    pub(super) ranges: [Range<u32>; 3],
    pub(super) truncated: Vec<u32>,
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
        let Some(core) = clip(&self.bounds, mask.as_ref(), self.info) else {
            return Ok(None);
        };
        let margin = margin(
            &core,
            self.segmenter.context(),
            self.segmenter.voxel_budget(),
        );
        let ranges = expand(&core, margin, self.info);
        let plan = self
            .segmenter
            .plan(ranges[2].end - ranges[2].start, self.ledger.spare())
            .map_err(|error| error.to_string())?;
        let Some(_lease) = self.ledger.claim(Consumer::Inference, plan.bytes()) else {
            return Err(format!(
                "the segmentation plan cannot claim the {:.1} GiB of graphics memory it needs",
                plan.bytes() as f64 / (1u64 << 30) as f64,
            ));
        };
        grana_log::info!(
            target: "grana::segment",
            peak = plan.bytes(),
            measured = self.ledger.measured(),
            allowance = self.ledger.total(),
            "the segmentation reserves the graphics memory its block needs"
        );
        let warp = self.request.drifts.warp(self.request.channel, time);
        let no_data = self.reader.pyramid().channels()[self.request.channel].no_data();
        let width = ranges[0].end - ranges[0].start;
        let height = ranges[1].end - ranges[1].start;
        let mut planes = Vec::with_capacity((ranges[2].end - ranges[2].start) as usize);
        let mut absents = Vec::with_capacity(planes.capacity());
        for z in ranges[2].clone() {
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
                let mask = plane
                    .iter()
                    .map(|value| !value.is_finite())
                    .collect::<Vec<_>>();
                for value in &mut plane {
                    if !value.is_finite() {
                        *value = 0.0;
                    }
                }
                absent = Some(mask);
            }
            planes.push(plane);
            absents.push(absent);
        }
        let mut volume = self
            .segmenter
            .segment(&planes, width, height, plan, progress)
            .map_err(|error| format!("the segmentation model failed: {error}"))?;
        volume.filter(|[x, y, z]| {
            let position = [
                ranges[0].start + x,
                ranges[1].start + y,
                ranges[2].start + z,
            ];
            let present = absents[z as usize]
                .as_ref()
                .is_none_or(|absent| !absent[y as usize * width as usize + x as usize]);
            let kept = core
                .iter()
                .zip(position)
                .all(|(range, value)| range.contains(&value));
            let inside = mask
                .as_ref()
                .is_none_or(|mask| mask.contains_cell(position));
            present && kept && inside
        });
        let origin = [0, 1, 2].map(|axis| core[axis].start - ranges[axis].start);
        let extent = [0, 1, 2].map(|axis| core[axis].end - core[axis].start);
        let volume = volume.cropped(origin, extent);
        let truncated = grana_analysis::truncated_objects(
            volume.labels(),
            [core[0].start, core[1].start, core[2].start],
            extent,
            extent,
            |cell| {
                if (0..3).any(|axis| cell[axis] < core[axis].start || cell[axis] >= core[axis].end)
                {
                    return true;
                }
                if mask.as_ref().is_some_and(|mask| !mask.contains_cell(cell)) {
                    return true;
                }
                let Some(absent) = absents[(cell[2] - ranges[2].start) as usize].as_ref() else {
                    return false;
                };
                absent[(cell[1] - ranges[1].start) as usize * width as usize
                    + (cell[0] - ranges[0].start) as usize]
            },
        );
        Ok(Some(Assembly {
            volume,
            ranges: core,
            truncated,
        }))
    }

    pub(super) fn surfaces(
        &self,
        time: u32,
        assembly: &Assembly,
    ) -> Result<Option<SurfaceSet>, String> {
        labelled::surfaces(labelled::Labelled {
            reader: self.reader,
            time,
            level: 0,
            ranges: &assembly.ranges,
            labels: assembly.volume.labels(),
            truncated: &assembly.truncated,
            drifts: &self.request.drifts,
            ids: &self.request.ids,
        })
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

fn margin(core: &[Range<u32>; 3], context: [u32; 3], budget: u64) -> [u32; 3] {
    let padded = |margin: [u32; 3]| {
        core.iter()
            .zip(margin)
            .map(|(range, margin)| u64::from(range.end - range.start + 2 * margin))
            .product::<u64>()
    };
    let mut margin = context;
    while margin != [0; 3] && padded(margin) > budget {
        margin = margin.map(|value| value / 2);
    }
    margin
}

fn expand(core: &[Range<u32>; 3], margin: [u32; 3], level: Level) -> [Range<u32>; 3] {
    let extent = level.extent();
    let extent = [extent.x(), extent.y(), extent.z()];
    std::array::from_fn(|axis| {
        core[axis].start.saturating_sub(margin[axis])
            ..core[axis]
                .end
                .saturating_add(margin[axis])
                .min(extent[axis])
    })
}
