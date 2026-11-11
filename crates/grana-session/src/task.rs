use std::sync::atomic::AtomicBool;

use grana_exec::{Executor, Job, Pipe, Role};

pub use grana_exec::Poll;

const CAPACITY: usize = 32;

pub struct Task<T> {
    events: Pipe<T>,
    job: Job,
}

impl<T: Send + 'static> Task<T> {
    pub fn spawn(
        role: Role,
        work: impl FnOnce(&AtomicBool, &mut dyn FnMut(T)) + Send + 'static,
    ) -> Self {
        let (sender, events) = Pipe::new(CAPACITY);
        let job = Executor::lane(role).submit(move |cancel| {
            let mut sink = |event: T| {
                sender.send_until(event, || cancel.cancelled());
            };
            work(cancel.flag(), &mut sink);
        });
        Self { events, job }
    }

    pub fn poll(&self) -> Poll<T> {
        self.events.poll()
    }
}

impl<T> Drop for Task<T> {
    fn drop(&mut self) {
        self.job.cancel();
    }
}

pub struct Progress<T> {
    task: Task<Update<T>>,
    done: usize,
    total: usize,
    steps: bool,
}

enum Update<T> {
    Step(usize, usize),
    Done(T),
}

impl<T: Send + 'static> Progress<T> {
    pub fn spawn(
        role: Role,
        work: impl FnOnce(&AtomicBool, &mut dyn FnMut(usize, usize)) -> T + Send + 'static,
    ) -> Self {
        let task = Task::spawn(role, move |cancel, sink| {
            let outcome = work(cancel, &mut |done, total| sink(Update::Step(done, total)));
            sink(Update::Done(outcome));
        });
        Self {
            task,
            done: 0,
            total: 0,
            steps: false,
        }
    }

    pub fn poll(&mut self) -> Poll<T> {
        loop {
            match self.task.poll() {
                Poll::Event(Update::Step(done, total)) => {
                    self.steps = true;
                    self.done = done;
                    self.total = total;
                }
                Poll::Event(Update::Done(outcome)) => return Poll::Event(outcome),
                Poll::Empty => return Poll::Empty,
                Poll::Closed => return Poll::Closed,
            }
        }
    }

    pub fn stepped(&mut self) -> bool {
        std::mem::take(&mut self.steps)
    }

    pub fn progress(&self) -> (usize, usize) {
        (self.done, self.total)
    }
}
