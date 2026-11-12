use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use grana_analysis::{CellSeed, GrowField, grow as flood};
use grana_object::{CellParams, CellSet, Roi, SurfaceSet};
use grana_scene::{Drift, ObjectFrame};
use grana_volume::Region;

use crate::channels::Channels;
use crate::event::Event;
use crate::run::domain::Domain;
use crate::run::labelled;
use crate::run::level::Leveling;

const GROW_VOXELS: u64 = 1 << 24;

pub struct GrowRequest {
    channel: usize,
    params: CellParams,
    region: Region,
    drift: Drift,
    roi: Option<Roi>,
    sources: Vec<Option<Arc<ObjectFrame>>>,
}

impl GrowRequest {
    pub fn new(
        channel: usize,
        params: CellParams,
        region: Region,
        drift: Drift,
        roi: Option<Roi>,
        sources: Vec<Option<Arc<ObjectFrame>>>,
    ) -> Self {
        Self {
            channel,
            params,
            region,
            drift,
            roi,
            sources,
        }
    }
}

pub fn grow(
    reader: &Channels,
    request: GrowRequest,
    cancel: &AtomicBool,
    sink: &mut dyn FnMut(Event),
) -> Result<(), String> {
    let pyramid = reader.pyramid();
    if request.channel >= pyramid.channel_count() {
        return Err(format!(
            "the dataset has {} channels but channel {} was requested",
            pyramid.channel_count(),
            request.channel
        ));
    }
    if pyramid.channels()[request.channel].intensity().span() <= 0.0 {
        return Err("the cell membrane channel has no intensity range".to_string());
    }
    let level = pyramid
        .fit_level(request.region, GROW_VOXELS)
        .ok_or_else(|| {
            "the dataset is too large to grow cells at any resolution level".to_string()
        })?;
    let info = pyramid.level(level);
    let ranges = request
        .region
        .voxel_range(info)
        .ok_or_else(|| "the cell growth region overlaps no voxel".to_string())?;
    let domain = Domain::new(request.region, info);
    let frames = pyramid.frame_count();
    let total = frames as usize;
    sink(Event::Progress {
        time: 0,
        done: 0,
        total,
    });
    for time in 0..frames {
        if cancel.load(Ordering::Relaxed) {
            return Ok(());
        }
        let Some(frame) = request
            .sources
            .get(time as usize)
            .and_then(Option::as_deref)
        else {
            continue;
        };
        let ObjectFrame::Surfaces(nuclei) = frame else {
            return Err("the cells grow from a surface object set".to_string());
        };
        if nuclei.is_empty() {
            continue;
        }
        let mask = domain.mask(request.roi.as_ref(), time)?;
        let warp = request.drift.warp(time);
        let leveling = Leveling::at(
            reader,
            time,
            request.channel,
            level,
            &ranges,
            mask.as_ref(),
            warp,
        )?;
        let values = reader
            .read_region(time, level, request.channel, &ranges, warp)
            .map_err(|error| error.to_string())?;
        let values = leveling.apply(&values);
        let seeds = frame
            .detections()
            .into_iter()
            .map(|detection| CellSeed::new(detection.centroid(), detection.radius()))
            .collect::<Vec<_>>();
        let Some(growth) = flood(
            GrowField {
                level: info,
                ranges: ranges.clone(),
                params: request.params,
                values: &values,
                mask: mask.as_ref(),
                seeds: &seeds,
            },
            cancel,
        ) else {
            sink(Event::Progress {
                time,
                done: time as usize + 1,
                total,
            });
            continue;
        };
        let Some(bodies) = labelled::surfaces(
            reader,
            time,
            level,
            info,
            growth.ranges(),
            growth.labels(),
            warp,
        )?
        else {
            sink(Event::Progress {
                time,
                done: time as usize + 1,
                total,
            });
            continue;
        };
        let cells = cells(nuclei, bodies, growth.nuclei());
        sink(Event::Frame {
            time,
            frame: ObjectFrame::Cells(cells),
        });
        sink(Event::Progress {
            time,
            done: time as usize + 1,
            total,
        });
    }
    if cancel.load(Ordering::Relaxed) {
        return Ok(());
    }
    sink(Event::Done);
    Ok(())
}

fn cells(nuclei: &SurfaceSet, bodies: SurfaceSet, grown: &[usize]) -> CellSet {
    assert_eq!(
        bodies.object_count(),
        grown.len(),
        "every grown body holds its nucleus"
    );
    let held = grown
        .iter()
        .map(|nucleus| nuclei.objects()[*nucleus])
        .collect::<Vec<_>>();
    CellSet::new(bodies, held)
}
