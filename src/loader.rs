use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};

use grana_render::BrickRequest;
use grana_volume::{BrickData, BrickGrid, Pyramid, Volume};
use grana_zarr::Image;

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
}

impl Loader {
    pub fn memory(volume: Volume) -> (Self, Pyramid) {
        let pyramid = volume.pyramid();
        let (requests, request_queue) = mpsc::channel::<BrickRequest>();
        let (event_sender, events) = mpsc::channel();
        std::thread::spawn(move || {
            let grid = BrickGrid::new(volume.extent());
            while let Ok(request) = request_queue.recv() {
                let data = volume.brick(&grid, request.coord);
                if event_sender.send(Event::Brick { request, data }).is_err() {
                    break;
                }
            }
        });
        (Self { events, requests }, pyramid)
    }

    pub fn file(path: PathBuf) -> Self {
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
                match image.read_brick(request.level, request.coord) {
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
        Self { events, requests }
    }

    pub fn poll(&self) -> Option<Event> {
        self.events.try_recv().ok()
    }

    pub fn request(&self, request: BrickRequest) -> bool {
        self.requests.send(request).is_ok()
    }
}
