use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use grana_exec::Cancel;

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

    pub(super) fn hold(&self, cancel: &Cancel) -> bool {
        if !self.paused() {
            return !cancel.cancelled();
        }
        let mut guard = self.lock.lock().unwrap_or_else(|error| error.into_inner());
        while self.paused() {
            if cancel.cancelled() {
                return false;
            }
            guard = self
                .ready
                .wait_timeout(guard, WAIT)
                .unwrap_or_else(|error| error.into_inner())
                .0;
        }
        !cancel.cancelled()
    }
}
