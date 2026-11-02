use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering as AtomicOrdering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};

use grana_image::{Format, Image, Source};
use grana_volume::{BrickData, BrickRequest, CHANNEL_LIMIT, Pyramid, VolumeSeries};

const ATTEMPTS: u8 = 3;
const WORKER_LIMIT: usize = 8;
const RETRY_PRIORITY: u64 = 0;

pub enum Event {
    Opened {
        name: String,
        pyramid: Pyramid,
        format: Format,
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
    queue: Arc<Queue>,
    time: Arc<AtomicU32>,
    source: Source,
}

struct Queue {
    state: Mutex<State>,
    ready: Condvar,
}

struct State {
    tasks: BinaryHeap<Task>,
    pending: HashMap<BrickRequest, Reservation>,
    priority: u64,
    closed: bool,
}

#[derive(Clone, Copy)]
struct Reservation {
    priority: u64,
    attempts: u8,
}

struct Task {
    priority: u64,
    request: BrickRequest,
}

impl PartialEq for Task {
    fn eq(&self, other: &Self) -> bool {
        self.priority == other.priority
    }
}

impl Eq for Task {}

impl Ord for Task {
    fn cmp(&self, other: &Self) -> Ordering {
        self.priority.cmp(&other.priority)
    }
}

impl PartialOrd for Task {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Queue {
    fn new() -> Self {
        Self {
            state: Mutex::new(State {
                tasks: BinaryHeap::new(),
                pending: HashMap::new(),
                priority: 0,
                closed: false,
            }),
            ready: Condvar::new(),
        }
    }

    fn submit(&self, requests: &[BrickRequest]) -> bool {
        let mut state = self.state.lock().expect("the loader queue is not poisoned");
        if state.closed {
            return false;
        }
        for request in requests.iter().rev() {
            state.priority += 1;
            let priority = state.priority;
            state.pending.insert(
                *request,
                Reservation {
                    priority,
                    attempts: 0,
                },
            );
            state.tasks.push(Task {
                priority,
                request: *request,
            });
        }
        drop(state);
        self.ready.notify_all();
        true
    }

    fn take(&self, time: u32) -> Option<(BrickRequest, u8)> {
        let mut state = self.state.lock().expect("the loader queue is not poisoned");
        loop {
            if state.closed {
                return None;
            }
            let Some(task) = state.tasks.pop() else {
                state = self
                    .ready
                    .wait(state)
                    .expect("the loader queue is not poisoned");
                continue;
            };
            let matches = state
                .pending
                .get(&task.request)
                .is_some_and(|reservation| reservation.priority == task.priority);
            if !matches {
                continue;
            }
            let reservation = state
                .pending
                .remove(&task.request)
                .expect("the reservation is ready");
            if task.request.time < time {
                continue;
            }
            return Some((task.request, reservation.attempts));
        }
    }

    fn retry(&self, request: BrickRequest, attempts: u8) -> bool {
        let mut state = self.state.lock().expect("the loader queue is not poisoned");
        if state.closed || attempts + 1 >= ATTEMPTS {
            return false;
        }
        state.pending.insert(
            request,
            Reservation {
                priority: RETRY_PRIORITY,
                attempts: attempts + 1,
            },
        );
        state.tasks.push(Task {
            priority: RETRY_PRIORITY,
            request,
        });
        drop(state);
        self.ready.notify_all();
        true
    }

    fn prune(&self, time: u32) {
        let mut state = self.state.lock().expect("the loader queue is not poisoned");
        state.pending.retain(|request, _| request.time >= time);
    }

    fn close(&self) {
        let mut state = self.state.lock().expect("the loader queue is not poisoned");
        state.closed = true;
        drop(state);
        self.ready.notify_all();
    }
}

impl Loader {
    pub fn memory(series: VolumeSeries) -> (Self, Pyramid) {
        let source = Source::memory(series);
        let reader = Image::open(&source).expect("an in-memory volume opens");
        let pyramid = reader.pyramid().clone();
        let (sender, events) = mpsc::channel();
        let queue = Arc::new(Queue::new());
        let time = Arc::new(AtomicU32::new(0));
        spawn(Arc::new(reader), &queue, &time, &sender);
        (
            Self {
                events,
                queue,
                time,
                source,
            },
            pyramid,
        )
    }

    pub fn file(path: PathBuf) -> Self {
        let source = Source::file(path);
        let (sender, events) = mpsc::channel();
        let queue = Arc::new(Queue::new());
        let time = Arc::new(AtomicU32::new(0));
        let opened = source.clone();
        let opening_sender = sender.clone();
        let opening_queue = Arc::clone(&queue);
        let opening_time = Arc::clone(&time);
        std::thread::spawn(move || {
            let reader = match Image::open(&opened) {
                Ok(reader) => reader,
                Err(detail) => {
                    let _ = opening_sender.send(Event::Failed(detail.to_string()));
                    opening_queue.close();
                    return;
                }
            };
            let channels = reader.pyramid().channel_count();
            if channels > CHANNEL_LIMIT {
                let _ = opening_sender.send(Event::Failed(format!(
                    "the dataset has {channels} channels, grana displays at most {CHANNEL_LIMIT}"
                )));
                opening_queue.close();
                return;
            }
            let format = reader.format().expect("a file image declares its format");
            let opened = Event::Opened {
                name: reader.name().to_string(),
                pyramid: reader.pyramid().clone(),
                format,
            };
            if opening_sender.send(opened).is_err() {
                return;
            }
            spawn(
                Arc::new(reader),
                &opening_queue,
                &opening_time,
                &opening_sender,
            );
        });
        Self {
            events,
            queue,
            time,
            source,
        }
    }

    pub fn poll(&self) -> Option<Event> {
        self.events.try_recv().ok()
    }

    pub fn request(&self, requests: &[BrickRequest]) -> bool {
        self.queue.submit(requests)
    }

    pub fn set_time(&self, time: u32) {
        self.time.store(time, AtomicOrdering::Relaxed);
        self.queue.prune(time);
    }

    pub fn source(&self) -> Source {
        self.source.clone()
    }
}

impl Drop for Loader {
    fn drop(&mut self) {
        self.queue.close();
    }
}

fn spawn(reader: Arc<Image>, queue: &Arc<Queue>, time: &Arc<AtomicU32>, events: &Sender<Event>) {
    for _ in 0..workers() {
        let reader = Arc::clone(&reader);
        let queue = Arc::clone(queue);
        let time = Arc::clone(time);
        let events = events.clone();
        std::thread::spawn(move || serve(reader, queue, time, events));
    }
}

fn serve(reader: Arc<Image>, queue: Arc<Queue>, time: Arc<AtomicU32>, events: Sender<Event>) {
    while let Some((request, attempts)) = queue.take(time.load(AtomicOrdering::Relaxed)) {
        match reader.read_brick(request.time, request.level, request.coord) {
            Ok(data) => {
                if events.send(Event::Brick { request, data }).is_err() {
                    return;
                }
            }
            Err(detail) => {
                if queue.retry(request, attempts) {
                    continue;
                }
                if events
                    .send(Event::ReadFailed {
                        request,
                        detail: detail.to_string(),
                    })
                    .is_err()
                {
                    return;
                }
            }
        }
    }
}

fn workers() -> usize {
    let available = std::thread::available_parallelism().map_or(1, |count| count.get());
    (available - 1).clamp(1, WORKER_LIMIT)
}
