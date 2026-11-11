use std::sync::atomic::AtomicBool;

use grana_exec::{Executor, Job, Pipe};

pub use grana_exec::Poll;

const CAPACITY: usize = 32;

pub struct Task<T> {
    events: Pipe<T>,
    job: Job,
}

impl<T: Send + 'static> Task<T> {
    pub fn spawn(work: impl FnOnce(&AtomicBool, &mut dyn FnMut(T)) + Send + 'static) -> Self {
        let (sender, events) = Pipe::new(CAPACITY);
        let job = Executor::shared().submit(move |cancel| {
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
