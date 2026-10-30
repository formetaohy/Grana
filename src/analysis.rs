use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};

use grana_analysis::{Spot, SpotParams, SpotScan, SpotSet};
use grana_volume::{Pyramid, VoxelData};
use grana_zarr::Image;

use crate::source::Source;

pub enum Event {
    Started { total: usize },
    Progress { done: usize, total: usize },
    Finished(SpotSet),
    Failed(String),
}

pub enum Poll {
    Event(Event),
    Empty,
    Closed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress {
    pub channel: usize,
    pub done: usize,
    pub total: usize,
}

pub struct Analysis {
    channel: usize,
    events: Receiver<Event>,
    cancel: Arc<AtomicBool>,
}

impl Analysis {
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

pub fn start(source: Source, channel: usize, params: SpotParams) -> Analysis {
    let cancel = Arc::new(AtomicBool::new(false));
    let token = Arc::clone(&cancel);
    let (events, receiver) = mpsc::channel();
    std::thread::spawn(move || run(source, channel, params, token, events));
    Analysis {
        channel,
        events: receiver,
        cancel,
    }
}

fn run(
    source: Source,
    channel: usize,
    params: SpotParams,
    cancel: Arc<AtomicBool>,
    events: Sender<Event>,
) {
    let result = match source {
        Source::Memory(volume) => {
            let pyramid = volume.pyramid();
            scan(&pyramid, channel, params, &cancel, &events, |range| {
                let extent = pyramid.level(0).extent();
                let data = volume.channels()[channel].data();
                Ok(data.region(extent, range[0].clone(), range[1].clone(), range[2].clone()))
            })
        }
        Source::File(path) => match Image::open(&path) {
            Ok(image) => {
                let pyramid = image.pyramid().clone();
                scan(&pyramid, channel, params, &cancel, &events, |range| {
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

fn scan(
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
    let _ = events.send(Event::Finished(SpotSet::new(channel, 0, spots)));
    Ok(())
}
