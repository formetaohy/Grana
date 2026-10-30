use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};

use grana_analysis::{SpotParams, SpotScan, SurfaceBuilder, SurfaceParams};
use grana_object::{Spot, SpotSet, SurfaceSet};
use grana_volume::{BrickCoord, Pyramid, VoxelData};
use grana_zarr::Image;

use crate::source::Source;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Spots,
    Surfaces,
}

pub enum Outcome {
    Spots(SpotSet),
    Surfaces(SurfaceSet),
}

pub enum Event {
    Started { total: usize },
    Progress { done: usize, total: usize },
    Finished(Outcome),
    Failed(String),
}

pub enum Poll {
    Event(Event),
    Empty,
    Closed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress {
    pub kind: Kind,
    pub channel: usize,
    pub done: usize,
    pub total: usize,
}

pub struct Analysis {
    kind: Kind,
    channel: usize,
    events: Receiver<Event>,
    cancel: Arc<AtomicBool>,
}

impl Analysis {
    pub fn kind(&self) -> Kind {
        self.kind
    }

    pub fn channel(&self) -> usize {
        self.channel
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

pub fn start_spots(source: Source, channel: usize, params: SpotParams) -> Analysis {
    let cancel = Arc::new(AtomicBool::new(false));
    let token = Arc::clone(&cancel);
    let (events, receiver) = mpsc::channel();
    std::thread::spawn(move || run_spots(source, channel, params, token, events));
    Analysis {
        kind: Kind::Spots,
        channel,
        events: receiver,
        cancel,
    }
}

pub fn start_surfaces(source: Source, channel: usize, params: SurfaceParams) -> Analysis {
    let cancel = Arc::new(AtomicBool::new(false));
    let token = Arc::clone(&cancel);
    let (events, receiver) = mpsc::channel();
    std::thread::spawn(move || run_surfaces(source, channel, params, token, events));
    Analysis {
        kind: Kind::Surfaces,
        channel,
        events: receiver,
        cancel,
    }
}

fn run_spots(
    source: Source,
    channel: usize,
    params: SpotParams,
    cancel: Arc<AtomicBool>,
    events: Sender<Event>,
) {
    let result = match source {
        Source::Memory(volume) => {
            let pyramid = volume.pyramid();
            scan_spots(&pyramid, channel, params, &cancel, &events, |range| {
                let extent = pyramid.level(0).extent();
                let data = volume.channels()[channel].data();
                Ok(data.region(extent, range[0].clone(), range[1].clone(), range[2].clone()))
            })
        }
        Source::File(path) => match Image::open(&path) {
            Ok(image) => {
                let pyramid = image.pyramid().clone();
                scan_spots(&pyramid, channel, params, &cancel, &events, |range| {
                    image
                        .read_region(
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
    };
    if let Err(detail) = result {
        let _ = events.send(Event::Failed(detail));
    }
}

fn run_surfaces(
    source: Source,
    channel: usize,
    params: SurfaceParams,
    cancel: Arc<AtomicBool>,
    events: Sender<Event>,
) {
    let result = match source {
        Source::Memory(volume) => {
            let pyramid = volume.pyramid();
            let extent = pyramid.level(0).extent();
            let grid = grana_volume::BrickGrid::new(extent);
            scan_surfaces(&pyramid, channel, params, &cancel, &events, |coord| {
                Ok(volume.channel_brick(&grid, coord, channel))
            })
        }
        Source::File(path) => match Image::open(&path) {
            Ok(image) => {
                let pyramid = image.pyramid().clone();
                scan_surfaces(&pyramid, channel, params, &cancel, &events, |coord| {
                    image
                        .read_channel_brick(0, channel, coord)
                        .map_err(|error| error.to_string())
                })
            }
            Err(error) => Err(error.to_string()),
        },
    };
    if let Err(detail) = result {
        let _ = events.send(Event::Failed(detail));
    }
}

fn scan_spots(
    pyramid: &Pyramid,
    channel: usize,
    params: SpotParams,
    cancel: &AtomicBool,
    events: &Sender<Event>,
    mut read: impl FnMut([Range<u32>; 3]) -> Result<VoxelData, String>,
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
    let _ = events.send(Event::Started { total });
    let mut values = Vec::new();
    let mut spots: Vec<Spot> = Vec::new();
    for index in 0..total {
        if cancel.load(Ordering::Relaxed) {
            return Ok(());
        }
        let range = scanner.read_range(index);
        let data = read(range)?;
        values.clear();
        values.resize(data.voxel_count(), 0.0);
        data.write_values(&mut values);
        spots.extend(scanner.detect(index, &values));
        let _ = events.send(Event::Progress {
            done: index + 1,
            total,
        });
    }
    let _ = events.send(Event::Finished(Outcome::Spots(SpotSet::new(
        channel, 0, spots,
    ))));
    Ok(())
}

fn scan_surfaces(
    pyramid: &Pyramid,
    channel: usize,
    params: SurfaceParams,
    cancel: &AtomicBool,
    events: &Sender<Event>,
    mut read: impl FnMut(BrickCoord) -> Result<VoxelData, String>,
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
    let mut builder = SurfaceBuilder::new(channel, 0, pyramid.level(0), params);
    let total = builder.coord_count();
    let _ = events.send(Event::Started { total });
    for index in 0..total {
        if cancel.load(Ordering::Relaxed) {
            return Ok(());
        }
        let coord = builder.coord(index);
        let values = read(coord)?;
        builder.add(coord, &values);
        if index % 16 == 0 || index + 1 == total {
            let _ = events.send(Event::Progress {
                done: index + 1,
                total,
            });
        }
    }
    let _ = events.send(Event::Finished(Outcome::Surfaces(builder.finish())));
    Ok(())
}
