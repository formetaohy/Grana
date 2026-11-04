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

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;
    use std::time::Duration;

    use super::Gate;

    #[test]
    fn holds_a_paused_worker_until_it_resumes_or_cancels() {
        let gate = Gate::new();
        let cancel = Arc::new(AtomicBool::new(false));
        gate.pause();
        let (sender, receiver) = mpsc::channel();
        let worker = {
            let gate = Arc::clone(&gate);
            let cancel = Arc::clone(&cancel);
            std::thread::spawn(move || {
                if gate.hold(&cancel) {
                    let _ = sender.send("resumed");
                }
            })
        };
        assert!(
            receiver.recv_timeout(Duration::from_millis(100)).is_err(),
            "a paused gate holds its worker"
        );
        assert!(gate.paused(), "the gate reports its pause");
        gate.resume();
        assert_eq!(
            receiver.recv_timeout(Duration::from_secs(5)).ok(),
            Some("resumed"),
            "a resumed gate releases its worker"
        );
        worker.join().expect("the paused worker finishes");
        gate.pause();
        cancel.store(true, Ordering::Relaxed);
        assert!(
            !gate.hold(&cancel),
            "a cancelled gate releases its paused worker"
        );
    }
}
