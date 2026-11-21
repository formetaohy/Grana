use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering as AtomicOrdering};
use std::sync::{Arc, RwLock};

use grana_exec::{Cancel, Executor, Pipe, Poll, Priority, Role, Sender};
use grana_image::{Format, Image, Plate, Series, Source};
use grana_pipeline::Channels;
use grana_scene::Drifts;
use grana_volume::{BrickRequest, CHANNEL_LIMIT, Fingerprint, PackedBrick, Pyramid, VolumeSeries};

const CAPACITY: usize = 16;

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
    pub imports_error: Option<String>,
    pub transfer: Option<Arc<grana_image::Transfer>>,
}

pub enum Event {
    Opened(Box<Opened>),
    Brick {
        generation: u64,
        request: BrickRequest,
        bytes: u64,
        data: PackedBrick,
    },
    ReadFailed {
        generation: u64,
        request: BrickRequest,
        detail: String,
        retryable: bool,
    },
    Failed(String),
}

pub struct Loader {
    events: Pipe<Event>,
    sender: Sender<Event>,
    reader: &'static Executor,
    shared: Arc<Shared>,
    source: Source,
}

struct Shared {
    time: AtomicU32,
    drifts: RwLock<Drifts>,
    channels: RwLock<Option<Arc<Channels>>>,
    generation: AtomicU64,
    closed: AtomicBool,
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
            imports_error: None,
            transfer: None,
        };
        let channels = Arc::new(Channels::stored(Arc::new(image)));
        let (sender, events) = Pipe::new(CAPACITY);
        (
            Self {
                events,
                sender,
                reader: Executor::lane(Role::Read),
                shared: Arc::new(Shared {
                    time: AtomicU32::new(0),
                    drifts: RwLock::new(Drifts::new()),
                    channels: RwLock::new(Some(channels)),
                    generation: AtomicU64::new(0),
                    closed: AtomicBool::new(false),
                }),
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
        let shared = Arc::new(Shared {
            time: AtomicU32::new(0),
            drifts: RwLock::new(Drifts::new()),
            channels: RwLock::new(None),
            generation: AtomicU64::new(0),
            closed: AtomicBool::new(false),
        });
        let reader = Executor::lane(Role::Read);
        let opening = source.clone();
        let opening_sender = sender.clone();
        let opening_shared = Arc::clone(&shared);
        reader.submit_with(Priority::Immediate, move |cancel| {
            let opened = grana_exec::contain(|| {
                open(opening, remote, &opening_shared, cancel, &opening_sender)
            });
            if let Err(fault) = opened {
                opening_sender
                    .send_until(Event::Failed(fault.describe("opening the dataset")), || {
                        cancel.cancelled()
                    });
            }
        });
        Self {
            events,
            sender,
            reader,
            shared,
            source,
        }
    }

    pub fn poll(&self) -> Option<Event> {
        match self.events.poll() {
            Poll::Event(event) => Some(event),
            Poll::Empty | Poll::Closed => None,
        }
    }

    pub fn generation(&self) -> u64 {
        self.shared.generation.load(AtomicOrdering::Relaxed)
    }

    pub fn invalidate(&self) {
        self.shared.generation.fetch_add(1, AtomicOrdering::Relaxed);
    }

    pub fn request(&self, demand: &[BrickRequest], prefetch: &[BrickRequest]) -> bool {
        if self.shared.closed.load(AtomicOrdering::Relaxed) {
            return false;
        }
        let current = self.shared.time.load(AtomicOrdering::Relaxed);
        let generation = self.generation();
        for (priority, requests) in [(Priority::Demand, demand), (Priority::Background, prefetch)] {
            for request in requests.iter().copied() {
                if request.time < current {
                    continue;
                }
                let shared = Arc::clone(&self.shared);
                let events = self.sender.clone();
                self.reader.submit_with(priority, move |cancel| {
                    let outcome =
                        grana_exec::contain(|| read(request, generation, &shared, cancel, &events));
                    if let Err(fault) = outcome {
                        events.send_until(
                            Event::ReadFailed {
                                generation,
                                request,
                                detail: fault.describe("reading a brick"),
                                retryable: false,
                            },
                            || cancel.cancelled(),
                        );
                    }
                });
            }
        }
        true
    }

    pub fn set_time(&self, time: u32) {
        self.shared.time.store(time, AtomicOrdering::Relaxed);
    }

    pub fn set_drifts(&self, drifts: Drifts) {
        *self
            .shared
            .drifts
            .write()
            .unwrap_or_else(|error| error.into_inner()) = drifts;
    }

    pub fn drifts(&self) -> Drifts {
        self.shared
            .drifts
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    pub fn source(&self) -> Source {
        self.source.clone()
    }

    pub fn set_channels(&self, channels: Channels) {
        *self
            .shared
            .channels
            .write()
            .unwrap_or_else(|error| error.into_inner()) = Some(Arc::new(channels));
    }

    pub fn channels(&self) -> Option<Arc<Channels>> {
        self.shared
            .channels
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }
}

impl Drop for Loader {
    fn drop(&mut self) {
        self.shared.closed.store(true, AtomicOrdering::Relaxed);
    }
}

fn open(source: Source, remote: bool, shared: &Shared, cancel: &Cancel, sender: &Sender<Event>) {
    let stopped = || shared.closed.load(AtomicOrdering::Relaxed) || cancel.cancelled();
    let fail = |detail: String| {
        sender.send_until(Event::Failed(detail), stopped);
    };
    let plate = match grana_image::plate(&source) {
        Ok(plate) => plate,
        Err(detail) => return fail(detail.to_string()),
    };
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
    let (imports, imports_error) = match (reader.family(), source.path()) {
        (Some(grana_readout::Family::Imaris), Some(path)) => match grana_readout::imported(path) {
            Ok(imported) => (imported, None),
            Err(detail) => (
                grana_readout::Imported::default(),
                Some(format!(
                    "the objects of {} could not be read: {detail}",
                    path.display()
                )),
            ),
        },
        _ => (grana_readout::Imported::default(), None),
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
        imports_error,
        transfer: reader.transfer(),
    }));
    if cancel.cancelled() || shared.closed.load(AtomicOrdering::Relaxed) {
        return;
    }
    let reader = Arc::new(Channels::stored(Arc::new(reader)));
    *shared
        .channels
        .write()
        .unwrap_or_else(|error| error.into_inner()) = Some(reader);
    sender.send_until(opened, stopped);
}

fn read(
    request: BrickRequest,
    generation: u64,
    shared: &Shared,
    cancel: &Cancel,
    events: &Sender<Event>,
) {
    let stopped = || {
        shared.closed.load(AtomicOrdering::Relaxed)
            || cancel.cancelled()
            || shared.generation.load(AtomicOrdering::Relaxed) != generation
            || request.time < shared.time.load(AtomicOrdering::Relaxed)
    };
    if stopped() {
        return;
    }
    let Some(channels) = shared
        .channels
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
    else {
        return;
    };
    let base = channels.pyramid().channel_count();
    let warps = shared
        .drifts
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .warps(request.time, base);
    match channels.read_brick(
        request.time,
        request.level,
        request.coord,
        &warps,
        request.channels,
    ) {
        Ok(data) => {
            let bytes = data.bytes();
            let packed = PackedBrick::of(&data, channels.pyramid(), request.channels);
            events.send_until(
                Event::Brick {
                    generation,
                    request,
                    bytes,
                    data: packed,
                },
                stopped,
            );
        }
        Err(error) => {
            events.send_until(
                Event::ReadFailed {
                    generation,
                    request,
                    detail: error.to_string(),
                    retryable: error.retryable(),
                },
                stopped,
            );
        }
    }
}
