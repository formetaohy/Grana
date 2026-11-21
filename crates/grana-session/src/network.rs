use std::sync::Arc;
use std::time::{Duration, Instant};

use grana_image::Transfer;

use crate::rate::Rate;
use crate::view::Network;

const IDLE: Duration = Duration::from_secs(1);

pub(crate) struct Watcher {
    handle: Option<Arc<Transfer>>,
    sampled: u64,
    carried: Option<Instant>,
    rate: Rate,
}

impl Watcher {
    pub(crate) fn new() -> Self {
        Self {
            handle: None,
            sampled: 0,
            carried: None,
            rate: Rate::default(),
        }
    }

    pub(crate) fn watch(&mut self, handle: Option<Arc<Transfer>>) {
        self.handle = handle;
        self.sampled = 0;
        self.carried = None;
        self.rate = Rate::default();
    }

    pub(crate) fn sample(&mut self, now: Instant) {
        let Some(handle) = self.handle.as_ref() else {
            return;
        };
        let fetched = handle.fetched();
        if fetched > self.sampled {
            self.rate.record(fetched - self.sampled, now);
            self.sampled = fetched;
            self.carried = Some(now);
        } else if self.carried.is_some_and(|at| now.duration_since(at) > IDLE) {
            self.rate.reset();
            self.carried = None;
        }
    }

    pub(crate) fn view(&self) -> Option<Network> {
        let handle = self.handle.as_ref()?;
        Some(Network {
            fetched: handle.fetched(),
            resumed: handle.resumed(),
            rate: self.rate.rate(),
        })
    }
}
