use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

const WAIT: Duration = Duration::from_millis(25);

pub(super) struct Gate {
    paused: AtomicBool,
    lock: Mutex<()>,
    ready: Condvar,
}

impl Gate {
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self {
            paused: AtomicBool::new(false),
            lock: Mutex::new(()),
            ready: Condvar::new(),
        })
    }

    pub(super) fn pause(&self) {
        self.paused.store(true, Ordering::Relaxed);
    }

    pub(super) fn resume(&self) {
        if self.paused.swap(false, Ordering::Relaxed) {
            self.ready.notify_all();
        }
    }

    pub(super) fn paused(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
    }

    pub(super) fn hold(&self, cancel: &AtomicBool) -> bool {
        if !self.paused() {
            return !cancel.load(Ordering::Relaxed);
        }
        let mut guard = self.lock.lock().unwrap_or_else(|error| error.into_inner());
        while self.paused() {
            if cancel.load(Ordering::Relaxed) {
                return false;
            }
            guard = self
                .ready
                .wait_timeout(guard, WAIT)
                .unwrap_or_else(|error| error.into_inner())
                .0;
        }
        !cancel.load(Ordering::Relaxed)
    }
}
