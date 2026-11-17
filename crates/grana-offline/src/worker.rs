use grana_exec::{Cancel, Executor, Pipe, Poll, Role, contain};

use crate::offline::{Offline, Outcome};
use crate::task::Task;

const CAPACITY: usize = 64;

pub struct Handle {
    cancel: Cancel,
    events: Pipe<Event>,
}

impl Handle {
    pub fn cancel(&self) {
        self.cancel.trigger();
    }

    pub fn poll(&self) -> Poll<Event> {
        self.events.poll()
    }
}

pub enum Event {
    Progress { frame: u32, total: u32 },
    Finished { outcome: Outcome },
}

pub struct Worker {
    executor: Executor,
}

impl Worker {
    pub fn spawn() -> Self {
        Self {
            executor: Executor::new(Role::Compute, 1),
        }
    }

    pub fn submit(&mut self, task: Task) -> Handle {
        let (sender, events) = Pipe::new(CAPACITY);
        let job = self.executor.submit(move |cancel| {
            if let Err(fault) = contain(|| run(task, cancel, &sender)) {
                sender.send_until(
                    Event::Finished {
                        outcome: Outcome::Failed {
                            detail: fault.describe("the export"),
                        },
                    },
                    || cancel.cancelled(),
                );
            }
        });
        Handle {
            cancel: job.token(),
            events,
        }
    }
}

fn run(task: Task, cancel: &Cancel, sender: &grana_exec::Sender<Event>) {
    let stopped = || cancel.cancelled();
    let finish = |outcome: Outcome| {
        sender.send_until(Event::Finished { outcome }, stopped);
    };
    if cancel.cancelled() {
        return finish(Outcome::Cancelled);
    }
    if let Some(written) = crate::page::write(&task.plan) {
        sender.send_until(Event::Progress { frame: 0, total: 1 }, stopped);
        let outcome = match written {
            Ok(path) => {
                sender.send_until(Event::Progress { frame: 1, total: 1 }, stopped);
                Outcome::Complete { path }
            }
            Err(detail) => Outcome::Failed { detail },
        };
        return finish(outcome);
    }
    let mut context = match Offline::headless() {
        Ok(context) => context,
        Err(detail) => return finish(Outcome::Failed { detail }),
    };
    let outcome = context.run(task, cancel, &mut |frame, total| {
        sender.send_until(Event::Progress { frame, total }, stopped);
    });
    finish(outcome);
}
