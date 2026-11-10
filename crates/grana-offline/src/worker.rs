use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};

use crate::offline::{Offline, Outcome};
use crate::task::Task;

pub struct Handle {
    id: u64,
    cancel: Arc<AtomicBool>,
}

impl Handle {
    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

pub enum Event {
    Progress { id: u64, frame: u32, total: u32 },
    Finished { id: u64, outcome: Outcome },
}

struct Request {
    id: u64,
    task: Task,
    cancel: Arc<AtomicBool>,
}

pub struct Worker {
    requests: Sender<Request>,
    events: Receiver<Event>,
    next: u64,
}

impl Worker {
    pub fn spawn() -> Self {
        let (requests, queue) = mpsc::channel::<Request>();
        let (events, receiver) = mpsc::channel::<Event>();
        std::thread::spawn(move || serve(queue, events));
        Self {
            requests,
            events: receiver,
            next: 1,
        }
    }

    pub fn submit(&mut self, task: Task) -> Handle {
        let id = self.next;
        self.next += 1;
        let cancel = Arc::new(AtomicBool::new(false));
        let _ = self.requests.send(Request {
            id,
            task,
            cancel: Arc::clone(&cancel),
        });
        Handle { id, cancel }
    }

    pub fn drain(&mut self) -> (Vec<Event>, bool) {
        let mut events = Vec::new();
        loop {
            match self.events.try_recv() {
                Ok(event) => events.push(event),
                Err(TryRecvError::Empty) => return (events, true),
                Err(TryRecvError::Disconnected) => return (events, false),
            }
        }
    }
}

fn serve(requests: Receiver<Request>, events: Sender<Event>) {
    let mut offline: Option<Offline> = None;
    while let Ok(Request { id, task, cancel }) = requests.recv() {
        if cancel.load(Ordering::Relaxed) {
            let _ = events.send(Event::Finished {
                id,
                outcome: Outcome::Cancelled,
            });
            continue;
        }
        if offline.is_none() {
            match Offline::headless() {
                Ok(context) => offline = Some(context),
                Err(detail) => {
                    let _ = events.send(Event::Finished {
                        id,
                        outcome: Outcome::Failed { detail },
                    });
                    continue;
                }
            }
        }
        let context = offline.as_mut().expect("the offline context is ready");
        let mut progress = |frame, total| {
            let _ = events.send(Event::Progress { id, frame, total });
        };
        let outcome = context.run(task, &cancel, &mut progress);
        offline = None;
        let _ = events.send(Event::Finished { id, outcome });
    }
}
