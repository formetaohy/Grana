use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use grana_analysis::{
    ColocPermutation, ColocScan, ColocScanConfig, FilamentScan, Roi, Sources, SpotScanner,
    SurfaceBuilder, TrackBuilder, coloc_levels, filament_level, relate_frames, surface_level,
};
use grana_geometry::MeshStore;
use grana_image::Image;
use grana_measure::{ColocPair, ColocSettings};
use grana_scene::{ObjectFrame, ObjectParams};
use grana_volume::VoxelData;

use crate::event::Event;

pub struct DetectRequest {
    channel: usize,
    params: ObjectParams,
    region: grana_volume::Region,
    roi: Option<Roi>,
    seeds: Vec<Vec<[f32; 3]>>,
    workers: usize,
    store: Arc<MeshStore>,
}

impl DetectRequest {
    pub fn new(
        channel: usize,
        params: ObjectParams,
        region: grana_volume::Region,
        roi: Option<Roi>,
        seeds: Vec<Vec<[f32; 3]>>,
        workers: usize,
        store: Arc<MeshStore>,
    ) -> Self {
        Self {
            channel,
            params,
            region,
            roi,
            seeds,
            workers,
            store,
        }
    }
}

pub(crate) fn detect(
    reader: &Image,
    request: DetectRequest,
    cancel: &AtomicBool,
    sink: &mut dyn FnMut(Event),
) -> Result<(), String> {
    match request.params {
        ObjectParams::Spots(params) => spots(reader, params, request, cancel, sink),
        ObjectParams::Surfaces(params) => surfaces(reader, params, request, cancel, sink),
        ObjectParams::Filaments(params) => filaments(reader, params, request, cancel, sink),
    }
}

pub(crate) fn tracks(
    frames: Vec<Vec<[f32; 3]>>,
    params: grana_analysis::TrackParams,
    frame_interval: f32,
    cancel: &AtomicBool,
    sink: &mut dyn FnMut(Event),
) {
    let total = frames.len();
    sink(Event::Progress {
        time: 0,
        done: 0,
        total,
    });
    let mut builder = TrackBuilder::new(params);
    for (time, centroids) in frames.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        builder.push(time as u32, centroids);
        if time % 8 == 0 || time + 1 == total {
            sink(Event::Progress {
                time: time as u32,
                done: time + 1,
                total,
            });
        }
    }
    if cancel.load(Ordering::Relaxed) {
        return;
    }
    sink(Event::Tracks(builder.finish(frame_interval)));
    sink(Event::Done);
}

pub(crate) fn relation(
    source: Vec<Option<Arc<ObjectFrame>>>,
    target: Vec<Option<Arc<ObjectFrame>>>,
    workers: usize,
    cancel: &AtomicBool,
    sink: &mut dyn FnMut(Event),
) {
    let total = source.len().min(target.len());
    sink(Event::Progress {
        time: 0,
        done: 0,
        total,
    });
    let counts = (0..total)
        .map(|time| source[time].as_deref().map(ObjectFrame::object_count))
        .collect::<Vec<_>>();
    let frames = match relate_frames(
        &counts,
        |time| shape(source.get(time).and_then(Option::as_deref)),
        |time| shape(target.get(time).and_then(Option::as_deref)),
        workers,
        cancel,
        &mut |done, total| {
            sink(Event::Progress {
                time: done as u32,
                done,
                total,
            });
        },
    ) {
        Ok(Some(frames)) => frames,
        Ok(None) => return,
        Err(detail) => {
            sink(Event::Failed(detail));
            return;
        }
    };
    for (time, frame) in frames.into_iter().enumerate() {
        let Some(frame) = frame else {
            continue;
        };
        sink(Event::Relation {
            time: time as u32,
            frame,
        });
    }
    sink(Event::Done);
}

fn shape(frame: Option<&ObjectFrame>) -> Result<Option<grana_analysis::Shape>, String> {
    let Some(frame) = frame else {
        return Ok(None);
    };
    frame.shape().map(Some).map_err(|error| error.to_string())
}

pub(crate) struct ColocRequest {
    pair: ColocPair,
    region: grana_volume::Region,
    roi: Option<Roi>,
    mask: Option<u64>,
    settings: ColocSettings,
}

impl ColocRequest {
    pub(crate) fn new(
        pair: ColocPair,
        region: grana_volume::Region,
        roi: Option<Roi>,
        mask: Option<u64>,
        settings: ColocSettings,
    ) -> Self {
        Self {
            pair,
            region,
            roi,
            mask,
            settings,
        }
    }
}

pub(crate) fn coloc(
    reader: &Image,
    request: ColocRequest,
    cancel: &AtomicBool,
    sink: &mut dyn FnMut(Event),
) -> Result<(), String> {
    let ColocRequest {
        pair,
        region,
        roi,
        mask: mask_id,
        settings,
    } = request;
    let pyramid = reader.pyramid();
    let channels = pyramid.channel_count();
    if pair.a() >= channels || pair.b() >= channels {
        return Err(format!(
            "the dataset has {channels} channels but the pair names {} and {}",
            pair.a(),
            pair.b()
        ));
    }
    let intensities = [pair.a(), pair.b()].map(|channel| pyramid.channels()[channel].intensity());
    if intensities.iter().any(|intensity| intensity.span() <= 0.0) {
        return Err("the selected channels have no intensity range to colocalize".to_string());
    }
    if region.enabled() && region.voxel_range(pyramid.level(0)).is_none() {
        return Err("the region does not overlap the dataset".to_string());
    }
    let frames = pyramid.frame_count();
    let levels = coloc_levels(pyramid, region, frames);
    let scan_level = levels.scan();
    let permutation_level = levels.permutation();
    let scan_info = pyramid.level(scan_level);
    let permutation_frames = levels.frames().to_vec();
    let mut scan = ColocScan::new(
        scan_info,
        &levels,
        ColocScanConfig::new(pair, channels, intensities, mask_id, frames),
        region,
    );
    let mut permutation =
        ColocPermutation::new(pyramid, region, pair, &levels, settings, intensities);
    let scan_tiles = scan.tile_count();
    let permutation_tiles = permutation.tile_count();
    let piggyback = permutation_level == scan_level
        && permutation_frames
            .iter()
            .enumerate()
            .all(|(time, sampled)| *sampled == time as u32)
        && permutation_frames.len() == frames as usize;
    let permutation_work = if piggyback {
        0
    } else {
        permutation_tiles * permutation_frames.len()
    };
    let total = scan_tiles * frames as usize + permutation_work;
    let mut done = 0usize;
    for time in 0..frames {
        let mask = roi
            .as_ref()
            .map(|roi| roi.mask(scan_info, time))
            .transpose()?;
        for index in 0..scan_tiles {
            if cancel.load(Ordering::Relaxed) {
                return Ok(());
            }
            let ranges = scan.tile(index);
            let first = reader.read_region(time, scan_level, pair.a(), &ranges)?;
            let second = reader.read_region(time, scan_level, pair.b(), &ranges)?;
            scan.add(time, index, &first, &second, mask.as_ref());
            if piggyback {
                permutation.add(time as usize, index, &first, &second, mask.as_ref());
            }
            done += 1;
            if index % 16 == 0 || index + 1 == scan_tiles {
                sink(Event::Progress { time, done, total });
            }
        }
    }
    if scan.voxels() == 0 {
        return Err("the analysis mask covers no voxels of the colocalization region".to_string());
    }
    if !piggyback {
        let permutation_info = pyramid.level(permutation_level);
        for (slot, time) in permutation_frames.iter().enumerate() {
            let mask = roi
                .as_ref()
                .map(|roi| roi.mask(permutation_info, *time))
                .transpose()?;
            for index in 0..permutation_tiles {
                if cancel.load(Ordering::Relaxed) {
                    return Ok(());
                }
                let ranges = permutation.tile(index);
                let first = reader.read_region(*time, permutation_level, pair.a(), &ranges)?;
                let second = reader.read_region(*time, permutation_level, pair.b(), &ranges)?;
                permutation.add(slot, index, &first, &second, mask.as_ref());
                done += 1;
                if index % 16 == 0 || index + 1 == permutation_tiles {
                    sink(Event::Progress {
                        time: *time,
                        done,
                        total,
                    });
                }
            }
        }
    }
    let permutation_voxels = permutation.sampled_voxels();
    let permutation_frame_count = permutation.sampled_frames();
    let blocks = permutation.blocks();
    let randomizations = permutation.randomizations();
    let significance = permutation.test();
    let series = scan.finish(
        significance,
        permutation_voxels,
        permutation_frame_count,
        randomizations,
        blocks,
    );
    sink(Event::Coloc {
        series: Box::new(series),
        region,
    });
    sink(Event::Done);
    Ok(())
}

fn spots(
    reader: &Image,
    params: grana_analysis::SpotParams,
    request: DetectRequest,
    cancel: &AtomicBool,
    sink: &mut dyn FnMut(Event),
) -> Result<(), String> {
    let DetectRequest {
        channel,
        region,
        roi,
        workers,
        ..
    } = request;
    let pyramid = reader.pyramid();
    if channel >= pyramid.channel_count() {
        return Err(format!(
            "the dataset has {} channels but channel {channel} was requested",
            pyramid.channel_count()
        ));
    }
    let span = pyramid.channels()[channel].intensity().span();
    if span <= 0.0 {
        return Err("the selected channel has no intensity range to detect".to_string());
    }
    let channels = pyramid.channel_count();
    let level = pyramid.level(0);
    let scanner = SpotScanner::new(0, level, span, params, region, channels, channel);
    let total = scanner.blocks();
    for time in 0..pyramid.frame_count() {
        if cancel.load(Ordering::Relaxed) {
            return Ok(());
        }
        let mask = roi.as_ref().map(|roi| roi.mask(level, time)).transpose()?;
        sink(Event::Progress {
            time,
            done: 0,
            total,
        });
        let read = |measured: usize, ranges: &[Range<u32>; 3]| {
            reader
                .read_region(time, 0, measured, ranges)
                .map_err(String::from)
        };
        let set = scanner.frame(workers, cancel, mask.as_ref(), &read, &mut |done, total| {
            sink(Event::Progress { time, done, total })
        })?;
        let Some(set) = set else {
            return Ok(());
        };
        sink(Event::Frame {
            time,
            frame: ObjectFrame::Spots(set),
        });
    }
    sink(Event::Done);
    Ok(())
}
fn surfaces(
    reader: &Image,
    params: grana_analysis::SurfaceParams,
    request: DetectRequest,
    cancel: &AtomicBool,
    sink: &mut dyn FnMut(Event),
) -> Result<(), String> {
    let DetectRequest {
        channel,
        region,
        roi,
        seeds,
        workers,
        store,
        ..
    } = request;
    let pyramid = reader.pyramid();
    if channel >= pyramid.channel_count() {
        return Err(format!(
            "the dataset has {} channels but channel {channel} was requested",
            pyramid.channel_count()
        ));
    }
    let span = pyramid.channels()[channel].intensity().span();
    if span <= 0.0 {
        return Err("the selected channel has no intensity range to detect".to_string());
    }
    let level_index = surface_level(pyramid, region).ok_or_else(|| {
        "the dataset is too large to detect surfaces at any resolution level".to_string()
    })?;
    let level = pyramid.level(level_index);
    for time in 0..pyramid.frame_count() {
        if cancel.load(Ordering::Relaxed) {
            return Ok(());
        }
        let builder = SurfaceBuilder::new(
            level_index,
            level,
            params,
            pyramid.channel_count(),
            channel,
            region,
            pyramid.channels()[channel].intensity(),
        )
        .with_seeds(seeds.get(time as usize).map_or(&[], Vec::as_slice));
        let total = builder.tile_count() * 2;
        let mask = roi.as_ref().map(|roi| roi.mask(level, time)).transpose()?;
        sink(Event::Progress {
            time,
            done: 0,
            total,
        });
        let set = builder.scan(
            workers,
            cancel,
            mask.as_ref(),
            &mut |done, total| sink(Event::Progress { time, done, total }),
            Sources::new(
                &|_index, ranges| {
                    reader
                        .read_region(time, level_index, channel, ranges)
                        .map_err(String::from)
                },
                &|measured, core| {
                    reader
                        .read_region(time, level_index, measured, core)
                        .map_err(String::from)
                },
                &store,
            ),
        )?;
        let Some(set) = set else {
            return Ok(());
        };
        sink(Event::Frame {
            time,
            frame: ObjectFrame::Surfaces(set),
        });
    }
    sink(Event::Done);
    Ok(())
}

fn filaments(
    reader: &Image,
    params: grana_analysis::FilamentParams,
    request: DetectRequest,
    cancel: &AtomicBool,
    sink: &mut dyn FnMut(Event),
) -> Result<(), String> {
    let DetectRequest {
        channel,
        region,
        roi,
        store,
        ..
    } = request;
    let pyramid = reader.pyramid();
    if channel >= pyramid.channel_count() {
        return Err(format!(
            "the dataset has {} channels but channel {channel} was requested",
            pyramid.channel_count()
        ));
    }
    if pyramid.channels()[channel].intensity().span() <= 0.0 {
        return Err("the selected channel has no intensity range to detect".to_string());
    }
    let level = filament_level(pyramid).ok_or_else(|| {
        "the dataset is too large to trace filaments at any resolution level".to_string()
    })?;
    let channels = pyramid.channel_count();
    let mut scan = FilamentScan::new(
        level,
        pyramid.level(level),
        params,
        channels,
        channel,
        region,
        pyramid.channels()[channel].intensity(),
    );
    let total = pyramid.frame_count();
    for time in 0..total {
        if cancel.load(Ordering::Relaxed) {
            return Ok(());
        }
        sink(Event::Progress {
            time,
            done: time as usize,
            total: total as usize,
        });
        let detection = full_region(reader, pyramid, time, level, channel)?;
        let mask = roi
            .as_ref()
            .map(|roi| roi.mask(pyramid.level(level), time))
            .transpose()?;
        let set = scan.frame(&detection, mask.as_ref(), &store, |measured| {
            full_region(reader, pyramid, time, level, measured)
        })?;
        sink(Event::Frame {
            time,
            frame: ObjectFrame::Filaments(set),
        });
        sink(Event::Progress {
            time,
            done: time as usize + 1,
            total: total as usize,
        });
    }
    sink(Event::Done);
    Ok(())
}

fn full_region(
    reader: &Image,
    pyramid: &grana_volume::Pyramid,
    time: u32,
    level: usize,
    channel: usize,
) -> Result<VoxelData, String> {
    let extent = pyramid.level(level).extent();
    reader
        .read_region(
            time,
            level,
            channel,
            &[0..extent.x(), 0..extent.y(), 0..extent.z()],
        )
        .map_err(String::from)
}
