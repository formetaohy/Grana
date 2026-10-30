use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};

use grana_render::BrickRequest;
use grana_volume::{BrickData, BrickGrid, Movie, Pyramid};
use grana_zarr::Image;

use crate::source::Source;

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
    pub fn memory(movie: Movie) -> (Self, Pyramid) {
        let pyramid = movie.pyramid().clone();
        let movie = Arc::new(movie);
        let worker = Arc::clone(&movie);
        let time = Arc::new(AtomicU32::new(0));
        let worker_time = Arc::clone(&time);
        let (requests, request_queue) = mpsc::channel::<BrickRequest>();
        let (event_sender, events) = mpsc::channel();
        std::thread::spawn(move || {
            let pyramid = worker.pyramid();
            let grid = BrickGrid::new(pyramid.level(0).extent());
            while let Ok(request) = request_queue.recv() {
                if request.time != worker_time.load(Ordering::Relaxed) {
                    continue;
                }
                let data = worker.brick(request.time, &grid, request.coord);
                if event_sender.send(Event::Brick { request, data }).is_err() {
                    break;
                }
            }
        });
        (
            Self {
                events,
                requests,
                time,
                source: Source::Memory(movie),
            },
            pyramid,
        )
    }

    pub fn file(path: PathBuf) -> Self {
        let source = Source::File(path.clone());
        let time = Arc::new(AtomicU32::new(0));
        let worker_time = Arc::clone(&time);
        let (requests, request_queue) = mpsc::channel::<BrickRequest>();
        let (event_sender, events) = mpsc::channel();
        std::thread::spawn(move || {
            let image = match Image::open(&path) {
                Ok(image) => image,
                Err(error) => {
                    let _ = event_sender.send(Event::Failed(error.to_string()));
                    return;
                }
            };
            if image.pyramid().channel_count() > grana_render::MAX_CHANNEL {
                let _ = event_sender.send(Event::Failed(format!(
                    "the dataset has {} channels, grana displays at most {}",
                    image.pyramid().channel_count(),
                    grana_render::MAX_CHANNEL
                )));
                return;
            }
            let opened = Event::Opened {
                name: image.name().to_string(),
                pyramid: image.pyramid().clone(),
            };
            if event_sender.send(opened).is_err() {
                return;
            }
            while let Ok(request) = request_queue.recv() {
                if request.time != worker_time.load(Ordering::Relaxed) {
                    continue;
                }
                match image.read_brick(request.time, request.level, request.coord) {
                    Ok(data) => {
                        if event_sender.send(Event::Brick { request, data }).is_err() {
                            return;
                        }
                    }
                    Err(error) => {
                        if event_sender
                            .send(Event::ReadFailed {
                                request,
                                detail: error.to_string(),
                            })
                            .is_err()
                        {
                            return;
                        }
                    }
                }
            }
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
