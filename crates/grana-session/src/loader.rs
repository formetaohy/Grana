use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};

use grana_volume::{BrickData, BrickRequest, CHANNEL_LIMIT, Pyramid, VolumeSeries};

use crate::Source;
use crate::reader::Reader;

pub enum Event {
    Opened {
        name: String,
        pyramid: Pyramid,
    },
    Brick {
        request: BrickRequest,
        data: BrickData,
    },
    ReadFailed {
        request: BrickRequest,
        detail: String,
    },
    Failed(String),
}

pub struct Loader {
    events: Receiver<Event>,
    requests: Sender<BrickRequest>,
    time: Arc<AtomicU32>,
    source: Source,
}

impl Loader {
    pub fn memory(series: VolumeSeries) -> (Self, Pyramid) {
        let source = Source::memory(series);
        let reader = Reader::open(&source).expect("an in-memory volume opens");
        let pyramid = reader.pyramid().clone();
        let loader = Self::spawn(source, reader);
        (loader, pyramid)
    }

    pub fn file(path: PathBuf) -> Self {
        let source = Source::file(path);
        let (event_sender, events) = mpsc::channel();
        let (requests, request_queue) = mpsc::channel();
        let time = Arc::new(AtomicU32::new(0));
        let worker_time = Arc::clone(&time);
        let worker_source = source.clone();
        std::thread::spawn(move || {
            let reader = match Reader::open(&worker_source) {
                Ok(reader) => reader,
                Err(detail) => {
                    let _ = event_sender.send(Event::Failed(detail));
                    return;
                }
            };
            let channels = reader.pyramid().channel_count();
            if channels > CHANNEL_LIMIT {
                let _ = event_sender.send(Event::Failed(format!(
                    "the dataset has {channels} channels, grana displays at most {CHANNEL_LIMIT}"
                )));
                return;
            }
            let name = reader.name().to_string();
            if event_sender
                .send(Event::Opened {
                    name,
                    pyramid: reader.pyramid().clone(),
                })
                .is_err()
            {
                return;
            }
            serve(reader, &request_queue, &worker_time, &event_sender);
        });
        Self {
            events,
            requests,
            time,
            source,
        }
    }

    fn spawn(source: Source, reader: Reader) -> Self {
        let (event_sender, events) = mpsc::channel();
        let (requests, request_queue) = mpsc::channel();
        let time = Arc::new(AtomicU32::new(0));
        let worker_time = Arc::clone(&time);
        std::thread::spawn(move || {
            serve(reader, &request_queue, &worker_time, &event_sender);
        });
        Self {
            events,
            requests,
            time,
            source,
        }
    }

    pub fn poll(&self) -> Option<Event> {
        self.events.try_recv().ok()
    }

    pub fn request(&self, request: BrickRequest) -> bool {
        self.requests.send(request).is_ok()
    }

    pub fn set_time(&self, time: u32) {
        self.time.store(time, Ordering::Relaxed);
    }

    pub fn source(&self) -> Source {
        self.source.clone()
    }
}

fn serve(
    reader: Reader,
    requests: &Receiver<BrickRequest>,
    time: &AtomicU32,
    events: &Sender<Event>,
) {
    while let Ok(request) = requests.recv() {
        if request.time < time.load(Ordering::Relaxed) {
            continue;
        }
        match reader.brick(request.time, request.coord) {
            Ok(data) => {
                if events.send(Event::Brick { request, data }).is_err() {
                    return;
                }
            }
            Err(detail) => {
                if events.send(Event::ReadFailed { request, detail }).is_err() {
                    return;
                }
            }
        }
    }
}
