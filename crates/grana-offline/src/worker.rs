use grana_exec::{Cancel, Executor, Pipe, Poll, Sender};

use crate::offline::{Offline, Outcome};
use crate::task::Task;

const CAPACITY: usize = 64;

pub struct Handle {
    id: u64,
    cancel: Cancel,
}

impl Handle {
    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn cancel(&self) {
        self.cancel.trigger();
    }
}

pub enum Event {
    Progress { id: u64, frame: u32, total: u32 },
    Finished { id: u64, outcome: Outcome },
}

pub struct Worker {
    events: Pipe<Event>,
    sender: Sender<Event>,
    executor: Executor,
    next: u64,
}

impl Worker {
    pub fn spawn() -> Self {
        let (sender, events) = Pipe::new(CAPACITY);
        Self {
            events,
            sender,
            executor: Executor::new(1),
            next: 1,
        }
    }

    pub fn submit(&mut self, task: Task) -> Handle {
        let id = self.next;
        self.next += 1;
        let sender = self.sender.clone();
        let job = self
            .executor
            .submit(move |cancel| run(id, task, cancel, &sender));
        Handle {
            id,
            cancel: job.token(),
        }
    }

    pub fn drain(&mut self) -> (Vec<Event>, bool) {
        let mut events = Vec::new();
        loop {
            match self.events.poll() {
                Poll::Event(event) => events.push(event),
                Poll::Empty => return (events, true),
                Poll::Closed => return (events, false),
            }
        }
    }
}

fn run(id: u64, task: Task, cancel: &Cancel, sender: &Sender<Event>) {
    let finish = |outcome: Outcome| {
        let _ = sender.send(Event::Finished { id, outcome });
    };
    if cancel.cancelled() {
        return finish(Outcome::Cancelled);
    }
    let mut context = match Offline::headless() {
        Ok(context) => context,
        Err(detail) => return finish(Outcome::Failed { detail }),
    };
    let outcome = context.run(task, cancel.flag(), &mut |frame, total| {
        let _ = sender.send(Event::Progress { id, frame, total });
    });
    finish(outcome);
}
