use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};

use grana_analysis::{
    FilamentParams, FilamentScan, SpotParams, SpotScan, SurfaceBuilder, SurfaceParams,
    TrackBuilder, TrackParams, filament_level,
};
use grana_object::{IntensityTable, SampleAccumulator, Spot, SpotSet, TrackSet};
use grana_scene::{Job, ObjectFrame, ObjectParams, ObjectSetId};
use grana_volume::{BrickCoord, BrickData, BrickGrid, Pyramid, VoxelData};
use grana_zarr::Image;

use crate::source::Source;

pub enum Event {
    Progress {
        time: u32,
        done: usize,
        total: usize,
    },
    Frame {
        time: u32,
        frame: ObjectFrame,
    },
    Tracks(TrackSet),
    Done,
    Failed(String),
}

pub enum Poll {
    Event(Event),
    Empty,
    Closed,
}

pub struct Analysis {
    set: ObjectSetId,
    job: Job,
    events: Receiver<Event>,
    cancel: Arc<AtomicBool>,
}

impl Analysis {
    pub fn set(&self) -> ObjectSetId {
        self.set
    }

    pub fn job(&self) -> Job {
        self.job
    }

    pub fn poll(&self) -> Poll {
        match self.events.try_recv() {
            Ok(event) => Poll::Event(event),
            Err(TryRecvError::Empty) => Poll::Empty,
            Err(TryRecvError::Disconnected) => Poll::Closed,
        }
    }

    fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl Drop for Analysis {
    fn drop(&mut self) {
        self.cancel();
    }
}

pub fn detect(source: Source, set: ObjectSetId, channel: usize, params: ObjectParams) -> Analysis {
    let cancel = Arc::new(AtomicBool::new(false));
    let token = Arc::clone(&cancel);
    let (events, receiver) = mpsc::channel();
    std::thread::spawn(move || run_detect(source, channel, params, token, events));
    Analysis {
        set,
        job: Job::Detect,
        events: receiver,
        cancel,
    }
}

pub fn track(
    set: ObjectSetId,
    frames: Vec<Vec<[f32; 3]>>,
    params: TrackParams,
    frame_interval: f32,
) -> Analysis {
    let cancel = Arc::new(AtomicBool::new(false));
    let token = Arc::clone(&cancel);
    let (events, receiver) = mpsc::channel();
    std::thread::spawn(move || run_tracks(frames, params, frame_interval, token, events));
    Analysis {
        set,
        job: Job::Track,
        events: receiver,
        cancel,
    }
}

fn run_detect(
    source: Source,
    channel: usize,
    params: ObjectParams,
    cancel: Arc<AtomicBool>,
    events: Sender<Event>,
) {
    let result = match params {
        ObjectParams::Spots(params) => detect_spots(source, channel, params, &cancel, &events),
        ObjectParams::Surfaces(params) => {
            detect_surfaces(source, channel, params, &cancel, &events)
        }
        ObjectParams::Filaments(params) => {
            detect_filaments(source, channel, params, &cancel, &events)
        }
    };
    if let Err(detail) = result {
        let _ = events.send(Event::Failed(detail));
    }
}

fn detect_spots(
    source: Source,
    channel: usize,
    params: SpotParams,
    cancel: &AtomicBool,
    events: &Sender<Event>,
) -> Result<(), String> {
    match source {
        Source::Memory(movie) => {
            let pyramid = movie.pyramid().clone();
            scan_spots(
                &pyramid,
                channel,
                params,
                cancel,
                events,
                |time, channel, range| {
                    Ok(movie.region(
                        time,
                        channel,
                        range[0].clone(),
                        range[1].clone(),
                        range[2].clone(),
                    ))
                },
            )
        }
        Source::File(path) => match Image::open(&path) {
            Ok(image) => {
                let pyramid = image.pyramid().clone();
                scan_spots(
                    &pyramid,
                    channel,
                    params,
                    cancel,
                    events,
                    |time, channel, range| {
                        image
                            .read_region(
                                time,
                                0,
                                channel,
                                range[0].clone(),
                                range[1].clone(),
                                range[2].clone(),
                            )
                            .map_err(|error| error.to_string())
                    },
                )
            }
            Err(error) => Err(error.to_string()),
        },
    }
}

fn detect_surfaces(
    source: Source,
    channel: usize,
    params: SurfaceParams,
    cancel: &AtomicBool,
    events: &Sender<Event>,
) -> Result<(), String> {
    match source {
        Source::Memory(movie) => {
            let pyramid = movie.pyramid().clone();
            let grid = BrickGrid::new(pyramid.level(0).extent());
            scan_surfaces(&pyramid, channel, params, cancel, events, |time, coord| {
                Ok(movie.brick(time, &grid, coord))
            })
        }
        Source::File(path) => match Image::open(&path) {
            Ok(image) => {
                let pyramid = image.pyramid().clone();
                scan_surfaces(&pyramid, channel, params, cancel, events, |time, coord| {
                    image
                        .read_brick(time, 0, coord)
                        .map_err(|error| error.to_string())
                })
            }
            Err(error) => Err(error.to_string()),
        },
    }
}

fn detect_filaments(
    source: Source,
    channel: usize,
    params: FilamentParams,
    cancel: &AtomicBool,
    events: &Sender<Event>,
) -> Result<(), String> {
    match source {
        Source::Memory(movie) => {
            let pyramid = movie.pyramid().clone();
            run_filaments(
                &pyramid,
                channel,
                params,
                cancel,
                events,
                |time, level, measured| {
                    let extent = pyramid.level(level).extent();
                    Ok(movie.region(time, measured, 0..extent.x(), 0..extent.y(), 0..extent.z()))
                },
            )
        }
        Source::File(path) => match Image::open(&path) {
            Ok(image) => {
                let pyramid = image.pyramid().clone();
                run_filaments(
                    &pyramid,
                    channel,
                    params,
                    cancel,
                    events,
                    |time, level, measured| {
                        let extent = pyramid.level(level).extent();
                        image
                            .read_region(
                                time,
                                level,
                                measured,
                                0..extent.x(),
                                0..extent.y(),
                                0..extent.z(),
                            )
                            .map_err(|error| error.to_string())
                    },
                )
            }
            Err(error) => Err(error.to_string()),
        },
    }
}

fn run_filaments(
    pyramid: &Pyramid,
    channel: usize,
    params: FilamentParams,
    cancel: &AtomicBool,
    events: &Sender<Event>,
    mut read: impl FnMut(u32, usize, usize) -> Result<VoxelData, String>,
) -> Result<(), String> {
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
    let mut scan = FilamentScan::new(level, pyramid.level(level), params, channels, channel);
    let total = pyramid.frame_count();
    for time in 0..total {
        if cancel.load(Ordering::Relaxed) {
            return Ok(());
        }
        let _ = events.send(Event::Progress {
            time,
            done: time as usize,
            total: total as usize,
        });
        let detection = read(time, level, channel)?;
        let set = scan.frame(&detection, |measured| read(time, level, measured))?;
        let _ = events.send(Event::Frame {
            time,
            frame: ObjectFrame::Filaments(set),
        });
        let _ = events.send(Event::Progress {
            time,
            done: time as usize + 1,
            total: total as usize,
        });
    }
    let _ = events.send(Event::Done);
    Ok(())
}

fn run_tracks(
    frames: Vec<Vec<[f32; 3]>>,
    params: TrackParams,
    frame_interval: f32,
    cancel: Arc<AtomicBool>,
    events: Sender<Event>,
) {
    let total = frames.len();
    let _ = events.send(Event::Progress {
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
            let _ = events.send(Event::Progress {
                time: time as u32,
                done: time + 1,
                total,
            });
        }
    }
    if cancel.load(Ordering::Relaxed) {
        return;
    }
    let _ = events.send(Event::Tracks(builder.finish(frame_interval)));
    let _ = events.send(Event::Done);
}

fn scan_spots(
    pyramid: &Pyramid,
    channel: usize,
    params: SpotParams,
    cancel: &AtomicBool,
    events: &Sender<Event>,
    mut read: impl FnMut(u32, usize, [Range<u32>; 3]) -> Result<VoxelData, String>,
) -> Result<(), String> {
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
    let mut scanner = SpotScan::new(pyramid.level(0), span, params);
    let total = scanner.block_count();
    let mut values = Vec::new();
    for time in 0..pyramid.frame_count() {
        if cancel.load(Ordering::Relaxed) {
            return Ok(());
        }
        let _ = events.send(Event::Progress {
            time,
            done: 0,
            total,
        });
        let mut spots: Vec<Spot> = Vec::new();
        let mut accumulators: Vec<SampleAccumulator> = Vec::new();
        for index in 0..total {
            if cancel.load(Ordering::Relaxed) {
                return Ok(());
            }
            let range = scanner.read_range(index);
            let start = spots.len();
            let data = read(time, channel, range.clone())?;
            values.clear();
            values.resize(data.voxel_count(), 0.0);
            data.write_values(&mut values);
            spots.extend(scanner.detect(index, &values));
            accumulators.resize(spots.len() * channels, SampleAccumulator::new());
            for (slot, accumulator) in scanner
                .measure(index, &spots[start..], &values)
                .into_iter()
                .enumerate()
            {
                accumulators[(start + slot) * channels + channel] = accumulator;
            }
            for measured in 0..channels {
                if measured == channel {
                    continue;
                }
                let data = read(time, measured, range.clone())?;
                values.clear();
                values.resize(data.voxel_count(), 0.0);
                data.write_values(&mut values);
                for (slot, accumulator) in scanner
                    .measure(index, &spots[start..], &values)
                    .into_iter()
                    .enumerate()
                {
                    accumulators[(start + slot) * channels + measured] = accumulator;
                }
            }
            if index % 8 == 0 || index + 1 == total {
                let _ = events.send(Event::Progress {
                    time,
                    done: index + 1,
                    total,
                });
            }
        }
        let intensities = accumulators
            .into_iter()
            .map(SampleAccumulator::statistics)
            .collect();
        let _ = events.send(Event::Frame {
            time,
            frame: ObjectFrame::Spots(SpotSet::new(
                0,
                spots,
                IntensityTable::new(channels, intensities),
            )),
        });
    }
    let _ = events.send(Event::Done);
    Ok(())
}

fn scan_surfaces(
    pyramid: &Pyramid,
    channel: usize,
    params: SurfaceParams,
    cancel: &AtomicBool,
    events: &Sender<Event>,
    mut read: impl FnMut(u32, BrickCoord) -> Result<BrickData, String>,
) -> Result<(), String> {
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
    for time in 0..pyramid.frame_count() {
        if cancel.load(Ordering::Relaxed) {
            return Ok(());
        }
        let mut builder = SurfaceBuilder::new(0, pyramid.level(0), params, channels, channel);
        let total = builder.coord_count();
        let _ = events.send(Event::Progress {
            time,
            done: 0,
            total,
        });
        for index in 0..total {
            if cancel.load(Ordering::Relaxed) {
                return Ok(());
            }
            let coord = builder.coord(index);
            let data = read(time, coord)?;
            builder.add(coord, &data);
            if index % 16 == 0 || index + 1 == total {
                let _ = events.send(Event::Progress {
                    time,
                    done: index + 1,
                    total,
                });
            }
        }
        let _ = events.send(Event::Frame {
            time,
            frame: ObjectFrame::Surfaces(builder.finish()),
        });
    }
    let _ = events.send(Event::Done);
    Ok(())
}
