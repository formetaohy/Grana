use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::channels::Channels;
use grana_analysis::{
    ColocPermutation, ColocScan, ColocScanConfig, FilamentScan, ObjectColocScan, RelationRun,
    SeedPolicy, Sources, SpotScanner, SurfaceBuilder, TrackBuilder, coloc_levels, filament_level,
    relate_frames, spot_level, surface_level,
};
use grana_geometry::GeometryStore;
use grana_measure::{ColocPair, ColocSettings, ObjectColoc, ObjectColocValue};
use grana_object::{LabelRaster, Roi};
use grana_scene::{Drift, ObjectFrame, ObjectParams};
use grana_volume::VoxelData;

use crate::event::Event;

mod calibration;
mod domain;
mod edit;
mod frames;
mod import;
mod interop;
mod mask;
mod segment;

pub use self::calibration::calibrate;
use self::domain::Domain;
pub use self::edit::edit;
use self::frames::Notice;
use self::frames::frames as pipeline;
pub use self::import::{ImportRequest, import};
pub use self::interop::{InteropRequest, interop};
pub use self::mask::{MaskRequest, mask};
pub use self::segment::{SegmentRequest, segment};

const FRAME_WORKERS: usize = 4;

const FILAMENT_BYTES: u64 = 512 << 20;

const FILAMENT_VOXEL_BYTES: u64 = 12;

fn frame_window(totals: u32, workers: usize, per_frame: usize) -> usize {
    (workers / per_frame.max(1)).max(1).min(totals as usize)
}

fn install(sink: &mut dyn FnMut(Event)) -> impl FnMut(Notice<ObjectFrame>) + '_ {
    move |notice| match notice {
        Notice::Progress { frame, done, total } => sink(Event::Progress {
            time: frame,
            done,
            total,
        }),
        Notice::Frame { frame, value } => sink(Event::Frame {
            time: frame,
            frame: value,
        }),
    }
}

pub struct DetectRequest {
    channel: usize,
    params: ObjectParams,
    region: grana_volume::Region,
    roi: Option<Roi>,
    seeds: Vec<Vec<[f32; 3]>>,
    seed_policy: SeedPolicy,
    drift: Drift,
    workers: usize,
    store: Arc<GeometryStore>,
}

impl DetectRequest {
    pub fn new(
        channel: usize,
        params: ObjectParams,
        region: grana_volume::Region,
        roi: Option<Roi>,
        drift: Drift,
        workers: usize,
        store: Arc<GeometryStore>,
    ) -> Self {
        Self {
            channel,
            params,
            region,
            roi,
            seeds: Vec::new(),
            seed_policy: SeedPolicy::Unseeded,
            drift,
            workers,
            store,
        }
    }

    pub fn with_seeds(mut self, seeds: Vec<Vec<[f32; 3]>>, policy: SeedPolicy) -> Self {
        self.seeds = seeds;
        self.seed_policy = policy;
        self
    }
}

pub fn detect(
    reader: &Channels,
    mut request: DetectRequest,
    cancel: &AtomicBool,
    sink: &mut dyn FnMut(Event),
) -> Result<(), String> {
    request.workers = grana_analysis::workers(request.workers);
    match request.params {
        ObjectParams::Spots(params) => spots(reader, params, request, cancel, sink),
        ObjectParams::Surfaces(params) => surfaces(reader, params, request, cancel, sink),
        ObjectParams::Filaments(params) => filaments(reader, params, request, cancel, sink),
    }
}

pub fn tracks(
    frames: Vec<Vec<[f32; 3]>>,
    params: grana_object::TrackParams,
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

#[derive(Clone, Copy, Debug)]
pub struct RelationRequest {
    pub shift: i32,
    pub same: bool,
    pub gate: f32,
    pub workers: usize,
}

pub fn relation(
    source: Vec<Option<Arc<ObjectFrame>>>,
    target: Vec<Option<Arc<ObjectFrame>>>,
    request: RelationRequest,
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
        RelationRun {
            shift: request.shift,
            gate: request.gate,
            same: request.same,
            workers: request.workers,
            cancel,
            progress: &mut |done, total| {
                sink(Event::Progress {
                    time: done as u32,
                    done,
                    total,
                });
            },
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

fn shape(frame: Option<&ObjectFrame>) -> Result<Option<grana_object::Shape>, String> {
    let Some(frame) = frame else {
        return Ok(None);
    };
    frame.shape().map(Some).map_err(|error| error.to_string())
}

pub struct ColocRequest {
    pair: ColocPair,
    region: grana_volume::Region,
    roi: Option<Roi>,
    mask: Option<u64>,
    settings: ColocSettings,
    drift: Drift,
    workers: usize,
}

impl ColocRequest {
    pub fn new(
        pair: ColocPair,
        region: grana_volume::Region,
        roi: Option<Roi>,
        mask: Option<u64>,
        settings: ColocSettings,
        drift: Drift,
        workers: usize,
    ) -> Self {
        Self {
            pair,
            region,
            roi,
            mask,
            settings,
            drift,
            workers,
        }
    }
}

pub fn coloc(
    reader: &Channels,
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
        drift,
        workers,
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
    let levels = coloc_levels(pyramid, region, frames, settings.precision());
    let scan_level = levels.scan();
    let permutation_level = levels.permutation();
    let scan_info = pyramid.level(scan_level);
    let permutation_info = pyramid.level(permutation_level);
    let permutation_frames = levels.frames().to_vec();
    let mut scan = ColocScan::new(
        scan_info,
        &levels,
        ColocScanConfig::new(pair, channels, intensities, mask_id, frames),
        region,
    );
    let mut permutation =
        ColocPermutation::new(pyramid, region, pair, &levels, settings, intensities);
    let scan_units = scan.tile_count() * frames as usize;
    let permutation_units = permutation.tile_count() * permutation_frames.len();
    let randomization_units = settings.permutations() as usize;
    let total = scan_units + permutation_units + randomization_units;
    let scan_domain = Domain::new(region, scan_info);
    let permutation_domain = Domain::new(region, permutation_info);
    let scanned = scan.scan(
        workers,
        cancel,
        &|time, channel, ranges| {
            reader
                .read_region(time, scan_level, channel, ranges, drift.offset(time))
                .map_err(String::from)
        },
        &|time| scan_domain.mask(roi.as_ref(), time),
        &mut |done, _| {
            sink(Event::Progress {
                time: 0,
                done,
                total,
            });
        },
    )?;
    if !scanned {
        return Ok(());
    }
    if scan.voxels() == 0 {
        return Err("the analysis mask covers no voxels of the colocalization region".to_string());
    }
    let loaded = permutation.load(
        &permutation_frames,
        workers,
        cancel,
        &|time, channel, ranges| {
            reader
                .read_region(time, permutation_level, channel, ranges, drift.offset(time))
                .map_err(String::from)
        },
        &|time| permutation_domain.mask(roi.as_ref(), time),
        &mut |done, _| {
            sink(Event::Progress {
                time: 0,
                done: scan_units + done,
                total,
            });
        },
    )?;
    if !loaded {
        return Ok(());
    }
    let permutation_voxels = permutation.sampled_voxels();
    let permutation_frame_count = permutation.sampled_frames();
    let blocks = permutation.blocks();
    let randomizations = permutation.randomizations();
    let significance = permutation.test(workers, cancel, &mut |done, _| {
        sink(Event::Progress {
            time: 0,
            done: scan_units + permutation_units + done,
            total,
        });
    });
    if cancel.load(Ordering::Relaxed) {
        return Ok(());
    }
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

pub struct ObjectColocRequest {
    pair: ColocPair,
    region: grana_volume::Region,
    frames: Vec<Option<Arc<ObjectFrame>>>,
    drift: Drift,
    workers: usize,
}

impl ObjectColocRequest {
    pub fn new(
        pair: ColocPair,
        region: grana_volume::Region,
        frames: Vec<Option<Arc<ObjectFrame>>>,
        drift: Drift,
        workers: usize,
    ) -> Self {
        Self {
            pair,
            region,
            frames,
            drift,
            workers,
        }
    }
}

pub fn object_coloc(
    reader: &Channels,
    request: ObjectColocRequest,
    cancel: &AtomicBool,
    sink: &mut dyn FnMut(Event),
) -> Result<(), String> {
    let ObjectColocRequest {
        pair,
        region,
        frames,
        drift,
        workers,
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
    let totals = pyramid.frame_count();
    if frames.len() != totals as usize {
        return Err(format!(
            "the object masks cover {} frames and the dataset holds {totals}",
            frames.len()
        ));
    }
    let level_index = grana_analysis::object_coloc_level(pyramid, totals, region, &|time| {
        frames
            .get(time as usize)
            .and_then(Option::as_ref)
            .map_or_else(Vec::new, |frame| frame.anchors())
    });
    let info = pyramid.level(level_index);
    let workers = grana_analysis::workers(workers);
    let window = frame_window(totals, workers, FRAME_WORKERS);
    let share = (workers / window).max(1);
    let mut collected: Vec<Option<Vec<ObjectColocValue>>> = vec![None; totals as usize];
    let completed = pipeline(
        totals,
        window,
        cancel,
        || (),
        |(), time, report| {
            let Some(frame) = frames.get(time as usize).and_then(Option::as_ref) else {
                return Ok(None);
            };
            let objects = frame.object_count();
            if objects == 0 {
                return Ok(None);
            }
            let anchors = frame.anchors();
            let scan = ObjectColocScan::new(
                pair,
                level_index,
                info,
                region,
                intensities,
                objects,
                &anchors,
            );
            let shape = frame.shape().map_err(|error| error.to_string())?;
            let raster = LabelRaster::new(&shape, info);
            let labels = |ranges: &[std::ops::Range<u32>; 3]| -> Result<Vec<u16>, String> {
                Ok(raster.render(ranges[0].clone(), ranges[1].clone(), ranges[2].clone()))
            };
            let read = |time: u32, channel: usize, ranges: &[std::ops::Range<u32>; 3]| {
                reader
                    .read_region(time, level_index, channel, ranges, drift.offset(time))
                    .map_err(String::from)
            };
            let mut notify = |done: usize, blocks: usize| report(done, blocks);
            let values = scan.slice(share, cancel, &labels, &read, time, &mut notify)?;
            Ok(values)
        },
        &mut |notice| match notice {
            Notice::Progress { frame, done, total } => sink(Event::Progress {
                time: frame,
                done,
                total,
            }),
            Notice::Frame { frame, value } => collected[frame as usize] = Some(value),
        },
    )?;
    if !completed {
        return Ok(());
    }
    let frames = collected
        .into_iter()
        .map(Option::unwrap_or_default)
        .collect::<Vec<_>>();
    sink(Event::ObjectColoc {
        coloc: Box::new(ObjectColoc::new(pair, level_index, frames)),
    });
    sink(Event::Done);
    Ok(())
}

fn spots(
    reader: &Channels,
    params: grana_object::SpotParams,
    request: DetectRequest,
    cancel: &AtomicBool,
    sink: &mut dyn FnMut(Event),
) -> Result<(), String> {
    let DetectRequest {
        channel,
        region,
        roi,
        workers,
        drift,
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
    let level_index = spot_level(pyramid, region, params);
    let level = pyramid.level(level_index);
    let scanner = SpotScanner::new(level_index, level, span, params, region, channels, channel);
    let domain = Domain::new(region, level);
    let totals = pyramid.frame_count();
    let window = frame_window(totals, workers, FRAME_WORKERS);
    let share = (workers / window).max(1);
    let completed = pipeline(
        totals,
        window,
        cancel,
        || (),
        |(), time, report| {
            let mask = domain.mask(roi.as_ref(), time)?;
            let read = |measured: usize, ranges: &[Range<u32>; 3]| {
                reader
                    .read_region(time, level_index, measured, ranges, drift.offset(time))
                    .map_err(String::from)
            };
            let set = scanner.frame(share, cancel, mask.as_ref(), &read, &mut |done, total| {
                report(done, total)
            })?;
            Ok(set.map(ObjectFrame::Spots))
        },
        &mut install(sink),
    )?;
    if completed {
        sink(Event::Done);
    }
    Ok(())
}

fn surfaces(
    reader: &Channels,
    params: grana_object::SurfaceParams,
    request: DetectRequest,
    cancel: &AtomicBool,
    sink: &mut dyn FnMut(Event),
) -> Result<(), String> {
    let DetectRequest {
        channel,
        region,
        roi,
        seeds,
        seed_policy,
        drift,
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
    let domain = Domain::new(region, level);
    let totals = pyramid.frame_count();
    let window = frame_window(totals, workers, FRAME_WORKERS);
    let share = (workers / window).max(1);
    let completed = pipeline(
        totals,
        window,
        cancel,
        || (),
        |(), time, report| {
            let frame_seeds = seeds.get(time as usize).map_or(&[][..], Vec::as_slice);
            let policy = match seed_policy {
                SeedPolicy::Strict if frame_seeds.is_empty() => SeedPolicy::Unseeded,
                policy => policy,
            };
            let builder = SurfaceBuilder::new(
                level_index,
                level,
                params,
                pyramid.channel_count(),
                channel,
                region,
                pyramid.channels()[channel].intensity(),
            )
            .with_seeds(frame_seeds, policy);
            let mask = domain.mask(roi.as_ref(), time)?;
            let set = builder.scan(
                share,
                cancel,
                mask.as_ref(),
                &mut |done, total| report(done, total),
                Sources::new(
                    &|ranges| {
                        reader
                            .read_region(time, level_index, channel, ranges, drift.offset(time))
                            .map_err(String::from)
                    },
                    &|measured, core| {
                        reader
                            .read_region(time, level_index, measured, core, drift.offset(time))
                            .map_err(String::from)
                    },
                    &store,
                ),
            )?;
            Ok(set.map(ObjectFrame::Surfaces))
        },
        &mut install(sink),
    )?;
    if completed {
        sink(Event::Done);
    }
    Ok(())
}

fn filaments(
    reader: &Channels,
    params: grana_object::FilamentParams,
    request: DetectRequest,
    cancel: &AtomicBool,
    sink: &mut dyn FnMut(Event),
) -> Result<(), String> {
    let DetectRequest {
        channel,
        region,
        roi,
        drift,
        store,
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
    if pyramid.channels()[channel].intensity().span() <= 0.0 {
        return Err("the selected channel has no intensity range to detect".to_string());
    }
    let level = filament_level(pyramid).ok_or_else(|| {
        "the dataset is too large to trace filaments at any resolution level".to_string()
    })?;
    let channels = pyramid.channel_count();
    let info = pyramid.level(level);
    let domain = Domain::new(region, info);
    let extent = info.extent();
    let voxels = u64::from(extent.x()) * u64::from(extent.y()) * u64::from(extent.z());
    let held = (FILAMENT_BYTES / (voxels * FILAMENT_VOXEL_BYTES).max(1)).max(1) as usize;
    let totals = pyramid.frame_count();
    let window = frame_window(totals, workers, 1).min(held);
    let intensity = pyramid.channels()[channel].intensity();
    let completed = pipeline(
        totals,
        window,
        cancel,
        || FilamentScan::new(level, info, params, channels, channel, region, intensity),
        |scan, time, _report| {
            let detection = full_region(reader, pyramid, time, level, channel, drift.offset(time))?;
            let mask = domain.mask(roi.as_ref(), time)?;
            let set = scan.frame(&detection, mask.as_ref(), &store, |measured| {
                full_region(reader, pyramid, time, level, measured, drift.offset(time))
            })?;
            Ok(Some(ObjectFrame::Filaments(set)))
        },
        &mut |notice| match notice {
            Notice::Progress { frame, done, total } => sink(Event::Progress {
                time: frame,
                done,
                total,
            }),
            Notice::Frame { frame, value } => {
                sink(Event::Progress {
                    time: frame,
                    done: frame as usize + 1,
                    total: totals as usize,
                });
                sink(Event::Frame {
                    time: frame,
                    frame: value,
                });
            }
        },
    )?;
    if completed {
        sink(Event::Done);
    }
    Ok(())
}

fn full_region(
    reader: &Channels,
    pyramid: &grana_volume::Pyramid,
    time: u32,
    level: usize,
    channel: usize,
    shift: [f32; 3],
) -> Result<VoxelData, String> {
    let extent = pyramid.level(level).extent();
    reader
        .read_region(
            time,
            level,
            channel,
            &[0..extent.x(), 0..extent.y(), 0..extent.z()],
            shift,
        )
        .map_err(String::from)
}

pub struct DriftRequest {
    channel: usize,
    reference: u32,
}

impl DriftRequest {
    pub fn new(channel: usize, reference: u32) -> Self {
        Self { channel, reference }
    }
}

pub fn drift(
    reader: &Channels,
    request: DriftRequest,
    cancel: &AtomicBool,
    sink: &mut dyn FnMut(Event),
) -> Result<(), String> {
    let DriftRequest { channel, reference } = request;
    let pyramid = reader.pyramid();
    let frames = pyramid.frame_count();
    if frames < 2 {
        return Err("the dataset holds one frame to align".to_string());
    }
    if reference >= frames {
        return Err(format!(
            "the dataset holds {frames} frames but frame {reference} was requested as the reference"
        ));
    }
    if channel >= pyramid.channel_count() {
        return Err(format!(
            "the dataset has {} channels but channel {channel} was requested",
            pyramid.channel_count()
        ));
    }
    if pyramid.channels()[channel].intensity().span() <= 0.0 {
        return Err("the selected channel has no intensity range to align".to_string());
    }
    let level = grana_analysis::drift_level(pyramid)
        .ok_or_else(|| "the dataset is too small to align".to_string())?;
    let ranges = grana_analysis::drift_spans(pyramid, level);
    let dims = window_dims(&ranges);
    let total = frames as usize;
    sink(Event::Progress {
        time: 0,
        done: 0,
        total,
    });
    let anchor = grana_analysis::drift_window(
        &reader
            .read_region(reference, level, channel, &ranges, [0.0; 3])
            .map_err(String::from)?,
        &ranges,
    );
    let ratio = (1u64 << level) as f32;
    let fine = if level > 0 {
        let ranges = grana_analysis::drift_spans(pyramid, 0);
        let dims = window_dims(&ranges);
        let anchor = grana_analysis::drift_window(
            &reader
                .read_region(reference, 0, channel, &ranges, [0.0; 3])
                .map_err(String::from)?,
            &ranges,
        );
        Some((ranges, dims, anchor))
    } else {
        None
    };
    let mut offsets = Vec::with_capacity(total);
    for time in 0..frames {
        if cancel.load(Ordering::Relaxed) {
            return Ok(());
        }
        let offset = if time == reference {
            [0.0; 3]
        } else {
            let moving = grana_analysis::drift_window(
                &reader
                    .read_region(time, level, channel, &ranges, [0.0; 3])
                    .map_err(String::from)?,
                &ranges,
            );
            let coarse =
                grana_analysis::drift_shift(&anchor, &moving, dims).map(|value| value * ratio);
            match &fine {
                None => coarse,
                Some((ranges, dims, fine_anchor)) => {
                    let whole = coarse.map(f32::round);
                    let moving = grana_analysis::drift_window(
                        &reader
                            .read_region(time, 0, channel, ranges, whole)
                            .map_err(String::from)?,
                        ranges,
                    );
                    let residual = grana_analysis::drift_shift(fine_anchor, &moving, *dims);
                    [0, 1, 2].map(|axis| whole[axis] + residual[axis])
                }
            }
        };
        offsets.push(offset.map(|value| value.clamp(-Drift::LIMIT, Drift::LIMIT)));
        sink(Event::Progress {
            time,
            done: time as usize + 1,
            total,
        });
    }
    if cancel.load(Ordering::Relaxed) {
        return Ok(());
    }
    sink(Event::Drift(Drift::new(channel, reference, offsets)));
    sink(Event::Done);
    Ok(())
}

fn window_dims(ranges: &[Range<u32>; 3]) -> [usize; 3] {
    [0, 1, 2].map(|axis| (ranges[axis].end - ranges[axis].start) as usize)
}
