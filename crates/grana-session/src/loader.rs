use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering as AtomicOrdering};
use std::sync::{Arc, RwLock};

use grana_exec::{Cancel, Executor, Pipe, Poll, Role, Sender};
use grana_image::{Format, Image, Plate, Series, Source};
use grana_pipeline::Channels;
use grana_scene::Drift;
use grana_volume::{BrickData, BrickRequest, CHANNEL_LIMIT, Fingerprint, Pyramid, VolumeSeries};

const ATTEMPTS: u32 = 3;
const RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(20);
const CAPACITY: usize = 16;
const OPENING: u64 = u64::MAX;

pub struct Opened {
    pub name: String,
    pub pyramid: Pyramid,
    pub format: Option<Format>,
    pub series: Vec<Series>,
    pub selected: usize,
    pub fingerprint: Fingerprint,
    pub remote: bool,
    pub labels: Vec<String>,
    pub labels_error: Option<String>,
    pub plate: Option<Plate>,
    pub field: usize,
    pub imports: grana_readout::Imported,
}

pub enum Event {
    Opened(Box<Opened>),
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
    events: Pipe<Event>,
    sender: Sender<Event>,
    reader: &'static Executor,
    time: Arc<AtomicU32>,
    drift: Arc<RwLock<Drift>>,
    channels: Arc<RwLock<Option<Arc<Channels>>>>,
    closed: Arc<AtomicBool>,
    source: Source,
}

impl Loader {
    pub fn memory(series: VolumeSeries) -> (Self, Opened) {
        let source = Source::memory(series);
        let image = Image::open(&source).expect("an in-memory volume opens");
        let opened = Opened {
            name: image.name().to_string(),
            pyramid: image.pyramid().clone(),
            format: None,
            series: image.series().to_vec(),
            selected: image.selected(),
            fingerprint: image
                .fingerprint()
                .expect("an in-memory volume fingerprints"),
            remote: false,
            labels: Vec::new(),
            labels_error: None,
            plate: None,
            field: 0,
            imports: grana_readout::Imported::default(),
        };
        let channels = Arc::new(Channels::stored(Arc::new(image)));
        let (sender, events) = Pipe::new(CAPACITY);
        (
            Self {
                events,
                sender,
                reader: Executor::lane(Role::Read),
                time: Arc::new(AtomicU32::new(0)),
                drift: Arc::new(RwLock::new(Drift::default())),
                channels: Arc::new(RwLock::new(Some(channels))),
                closed: Arc::new(AtomicBool::new(false)),
                source,
            },
            opened,
        )
    }

    pub fn file(path: PathBuf) -> Self {
        Self::file_series(path, 0)
    }

    pub fn file_series(path: PathBuf, series: usize) -> Self {
        Self::open(Source::file_series(path, series))
    }

    pub fn remote(url: String) -> Self {
        Self::open(Source::remote(url))
    }

    pub fn remote_series(url: String, series: usize) -> Self {
        Self::open(Source::remote_series(url, series))
    }

    fn open(source: Source) -> Self {
        let remote = source.is_remote();
        let (sender, events) = Pipe::new(CAPACITY);
        let time = Arc::new(AtomicU32::new(0));
        let drift = Arc::new(RwLock::new(Drift::default()));
        let channels = Arc::new(RwLock::new(None));
        let closed = Arc::new(AtomicBool::new(false));
        let reader = Executor::lane(Role::Read);
        let opening = source.clone();
        let opening_sender = sender.clone();
        let opening_channels = Arc::clone(&channels);
        let opening_closed = Arc::clone(&closed);
        reader.submit_ordered(OPENING, move |cancel| {
            open(
                opening,
                remote,
                &opening_channels,
                &opening_closed,
                cancel,
                &opening_sender,
            );
        });
        Self {
            events,
            sender,
            reader,
            time,
            drift,
            channels,
            closed,
            source,
        }
    }

    pub fn poll(&self) -> Option<Event> {
        match self.events.poll() {
            Poll::Event(event) => Some(event),
            Poll::Empty | Poll::Closed => None,
        }
    }

    pub fn request(&self, requests: &[BrickRequest]) -> bool {
        if self.closed.load(AtomicOrdering::Relaxed) {
            return false;
        }
        for request in requests.iter().rev() {
            let request = *request;
            let priority = self.reader.stamp();
            let channels = Arc::clone(&self.channels);
            let time = Arc::clone(&self.time);
            let drift = Arc::clone(&self.drift);
            let closed = Arc::clone(&self.closed);
            let events = self.sender.clone();
            self.reader.submit_ordered(priority, move |cancel| {
                read(request, &channels, &time, &drift, &closed, cancel, &events);
            });
        }
        true
    }

    pub fn set_time(&self, time: u32) {
        self.time.store(time, AtomicOrdering::Relaxed);
    }

    pub fn set_drift(&self, drift: Drift) {
        *self
            .drift
            .write()
            .unwrap_or_else(|error| error.into_inner()) = drift;
    }

    pub fn drift(&self) -> Drift {
        self.drift
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    pub fn source(&self) -> Source {
        self.source.clone()
    }

    pub fn set_channels(&self, channels: Channels) {
        *self
            .channels
            .write()
            .unwrap_or_else(|error| error.into_inner()) = Some(Arc::new(channels));
    }

    pub fn channels(&self) -> Option<Arc<Channels>> {
        self.channels
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }
}

impl Drop for Loader {
    fn drop(&mut self) {
        self.closed.store(true, AtomicOrdering::Relaxed);
    }
}

fn open(
    source: Source,
    remote: bool,
    channels: &RwLock<Option<Arc<Channels>>>,
    closed: &AtomicBool,
    cancel: &Cancel,
    sender: &Sender<Event>,
) {
    let stopped = || closed.load(AtomicOrdering::Relaxed) || cancel.cancelled();
    let fail = |detail: String| {
        sender.send_until(Event::Failed(detail), stopped);
    };
    let plate = grana_image::plate(&source).ok().flatten();
    let source = match grana_image::resolve_series(&source) {
        Ok(source) => source,
        Err(detail) => return fail(detail.to_string()),
    };
    let field = plate.as_ref().map_or(0, |plate| {
        source
            .group()
            .and_then(|group| {
                plate
                    .fields()
                    .iter()
                    .position(|field| field.path() == group)
            })
            .unwrap_or(0)
    });
    let reader = match Image::open(&source) {
        Ok(reader) => reader,
        Err(detail) => return fail(detail.to_string()),
    };
    let count = reader.pyramid().channel_count();
    if count > CHANNEL_LIMIT {
        return fail(format!(
            "the dataset has {count} channels, grana displays at most {CHANNEL_LIMIT}"
        ));
    }
    let format = reader.format().expect("a file image declares its format");
    let fingerprint = match reader.fingerprint() {
        Ok(fingerprint) => fingerprint,
        Err(detail) => return fail(format!("the dataset cannot be fingerprinted: {detail}")),
    };
    let (labels, labels_error) = match reader.labels() {
        Ok(labels) => (labels.names().map(str::to_string).collect(), None),
        Err(detail) => (Vec::new(), Some(detail.to_string())),
    };
    let imports = match (format, source.path()) {
        (Format::Imaris, Some(path)) => grana_readout::imported(path).unwrap_or_default(),
        _ => grana_readout::Imported::default(),
    };
    let opened = Event::Opened(Box::new(Opened {
        name: reader.name().to_string(),
        pyramid: reader.pyramid().clone(),
        format: Some(format),
        series: reader.series().to_vec(),
        selected: reader.selected(),
        fingerprint,
        remote,
        labels,
        labels_error,
        plate,
        field,
        imports,
    }));
    if cancel.cancelled() || closed.load(AtomicOrdering::Relaxed) {
        return;
    }
    let reader = Arc::new(Channels::stored(Arc::new(reader)));
    *channels.write().unwrap_or_else(|error| error.into_inner()) = Some(reader);
    sender.send_until(opened, stopped);
}

fn read(
    request: BrickRequest,
    channels: &RwLock<Option<Arc<Channels>>>,
    time: &AtomicU32,
    drift: &RwLock<Drift>,
    closed: &AtomicBool,
    cancel: &Cancel,
    events: &Sender<Event>,
) {
    let stopped = || closed.load(AtomicOrdering::Relaxed) || cancel.cancelled();
    if stopped() {
        return;
    }
    if request.time < time.load(AtomicOrdering::Relaxed) {
        return;
    }
    let shift = drift
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .offset(request.time);
    let Some(channels) = channels
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
    else {
        return;
    };
    for attempt in 1..=ATTEMPTS {
        if stopped() {
            return;
        }
        match channels.read_brick(request.time, request.level, request.coord, shift) {
            Ok(data) => {
                events.send_until(Event::Brick { request, data }, stopped);
                return;
            }
            Err(detail) if attempt == ATTEMPTS => {
                events.send_until(
                    Event::ReadFailed {
                        request,
                        detail: detail.to_string(),
                    },
                    stopped,
                );
            }
            Err(_) => std::thread::sleep(RETRY_DELAY),
        }
    }
}
