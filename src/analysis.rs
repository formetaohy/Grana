use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};

use grana_analysis::{
    SpotParams, SpotScan, SurfaceBuilder, SurfaceParams, TrackBuilder, TrackParams,
};
use grana_object::{Spot, SpotSet, SurfaceSet, TrackSet};
use grana_volume::{BrickCoord, BrickGrid, Pyramid, VoxelData};
use grana_zarr::Image;

use crate::objects::{ObjectParams, ObjectSetId};
use crate::source::Source;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Job {
    Detect,
    Track,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress {
    pub set: ObjectSetId,
    pub job: Job,
    pub time: u32,
    pub done: usize,
    pub total: usize,
}

pub enum Outcome {
    Spots(SpotSet),
    Surfaces(SurfaceSet),
}

pub enum Event {
    Progress {
        time: u32,
        done: usize,
        total: usize,
    },
    Frame {
        time: u32,
        outcome: Outcome,
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
            scan_spots(&pyramid, channel, params, cancel, events, |time, range| {
                Ok(movie.region(
                    time,
                    channel,
                    range[0].clone(),
                    range[1].clone(),
                    range[2].clone(),
                ))
            })
        }
        Source::File(path) => match Image::open(&path) {
            Ok(image) => {
                let pyramid = image.pyramid().clone();
                scan_spots(&pyramid, channel, params, cancel, events, |time, range| {
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
                })
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
                Ok(movie.channel_brick(time, &grid, coord, channel))
            })
        }
        Source::File(path) => match Image::open(&path) {
            Ok(image) => {
                let pyramid = image.pyramid().clone();
                scan_surfaces(&pyramid, channel, params, cancel, events, |time, coord| {
                    image
                        .read_channel_brick(time, 0, channel, coord)
                        .map_err(|error| error.to_string())
                })
            }
            Err(error) => Err(error.to_string()),
        },
    }
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
    mut read: impl FnMut(u32, [Range<u32>; 3]) -> Result<VoxelData, String>,
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
        for index in 0..total {
            if cancel.load(Ordering::Relaxed) {
                return Ok(());
            }
            let range = scanner.read_range(index);
            let data = read(time, range)?;
            values.clear();
            values.resize(data.voxel_count(), 0.0);
            data.write_values(&mut values);
            spots.extend(scanner.detect(index, &values));
            if index % 8 == 0 || index + 1 == total {
                let _ = events.send(Event::Progress {
                    time,
                    done: index + 1,
                    total,
                });
            }
        }
        let _ = events.send(Event::Frame {
            time,
            outcome: Outcome::Spots(SpotSet::new(0, spots)),
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
    mut read: impl FnMut(u32, BrickCoord) -> Result<VoxelData, String>,
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
    for time in 0..pyramid.frame_count() {
        if cancel.load(Ordering::Relaxed) {
            return Ok(());
        }
        let mut builder = SurfaceBuilder::new(0, pyramid.level(0), params);
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
            let values = read(time, coord)?;
            builder.add(coord, &values);
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
            outcome: Outcome::Surfaces(builder.finish()),
        });
    }
    let _ = events.send(Event::Done);
    Ok(())
}
