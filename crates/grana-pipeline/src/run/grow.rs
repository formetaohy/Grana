use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use grana_analysis::{CellField, CellSeed, GrowField, Growth, cell_band, cell_level};
use grana_object::{CellParams, CellSet, Roi, SurfaceSet};
use grana_scene::{Drifts, ObjectFrame};
use grana_volume::Region;
use grana_warp::Warp;

use crate::channels::Channels;
use crate::event::Event;
use crate::run::domain::Domain;
use crate::run::labelled;
use crate::run::level::Leveling;

pub struct GrowRequest {
    channel: usize,
    params: CellParams,
    region: Region,
    drifts: Drifts,
    roi: Option<Roi>,
    sources: Vec<Option<Arc<ObjectFrame>>>,
}

impl GrowRequest {
    pub fn new(
        channel: usize,
        params: CellParams,
        region: Region,
        drifts: Drifts,
        roi: Option<Roi>,
        sources: Vec<Option<Arc<ObjectFrame>>>,
    ) -> Self {
        Self {
            channel,
            params,
            region,
            drifts,
            roi,
            sources,
        }
    }
}

struct Field<'a> {
    reader: &'a Channels,
    time: u32,
    level: usize,
    channel: usize,
    warp: &'a Warp,
    leveling: &'a Leveling,
}

impl CellField for Field<'_> {
    fn values(&self, ranges: &[Range<u32>; 3]) -> Result<Vec<f32>, String> {
        let values = self
            .reader
            .read_region(self.time, self.level, self.channel, ranges, self.warp)
            .map_err(|error| error.to_string())?;
        Ok(self.leveling.apply(&values))
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
    let mut radius = 0.0f32;
    for source in &request.sources {
        let Some(frame) = source.as_deref() else {
            continue;
        };
        if !matches!(frame, ObjectFrame::Surfaces(_)) {
            return Err("the cells grow from a surface object set".to_string());
        }
        for detection in frame.detections() {
            radius = radius.max(detection.radius());
        }
    }
    let frames = pyramid.frame_count();
    let total = frames as usize;
    sink(Event::Progress {
        time: 0,
        done: 0,
        total,
    });
    if !(radius.is_finite() && radius > 0.0) {
        if cancel.load(Ordering::Relaxed) {
            return Ok(());
        }
        sink(Event::Done);
        return Ok(());
    }
    let Some(level) = cell_level(pyramid, request.region, request.params, radius) else {
        return Err("the dataset is too large to grow cells at any resolution level".to_string());
    };
    let info = pyramid.level(level);
    let ranges = request
        .region
        .voxel_range(info)
        .ok_or_else(|| "the cell growth region overlaps no voxel".to_string())?;
    let band = cell_band(
        info,
        &ranges,
        request.params,
        request.params.reach() * radius,
    );
    let domain = Domain::new(request.region, info);
    let channels = pyramid.channel_count();
    let width = ranges[0].end - ranges[0].start;
    let height = ranges[1].end - ranges[1].start;
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
        let warp = request.drifts.warp(request.channel, time);
        let leveling = Leveling::at(
            reader,
            time,
            request.channel,
            level,
            &ranges,
            mask.as_ref(),
            warp,
        )?;
        let seeds = frame
            .detections()
            .into_iter()
            .map(|detection| CellSeed::new(detection.centroid(), detection.radius()))
            .collect::<Vec<_>>();
        let field = Field {
            reader,
            time,
            level,
            channel: request.channel,
            warp,
            leveling: &leveling,
        };
        let Some(mut growth) = Growth::new(
            GrowField {
                level: info,
                ranges: ranges.clone(),
                params: request.params,
                band,
                mask: mask.as_ref(),
                seeds: &seeds,
            },
            &field,
            cancel,
        )?
        else {
            sink(Event::Progress {
                time,
                done: time as usize + 1,
                total,
            });
            continue;
        };
        let mut surfaces = labelled::Surfaces::new(level, info, channels);
        let mut halted = false;
        for index in 0..growth.bands() {
            if cancel.load(Ordering::Relaxed) {
                halted = true;
                break;
            }
            let Some(band) = growth.band(index)? else {
                halted = true;
                break;
            };
            let extent = [
                ranges[0].clone(),
                ranges[1].clone(),
                band.start()..band.start() + band.planes(),
            ];
            let mut data = Vec::with_capacity(channels);
            for channel in 0..channels {
                data.push(
                    reader
                        .read_region(time, level, channel, &extent, warp)
                        .map_err(|error| error.to_string())?,
                );
            }
            surfaces.push(
                [ranges[0].start, ranges[1].start, band.start()],
                [width, height, band.core()],
                band.labels(),
                &data,
            );
        }
        if halted {
            return Ok(());
        }
        let Some(bodies) = surfaces.finish() else {
            sink(Event::Progress {
                time,
                done: time as usize + 1,
                total,
            });
            continue;
        };
        let cells = cells(nuclei, bodies, &growth.nuclei());
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
