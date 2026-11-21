use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Default)]
pub struct Transfer {
    fetched: AtomicU64,
    resumed: AtomicU64,
}

impl Transfer {
    pub fn record(&self, fetched: u64, resumed: u64) {
        self.fetched.fetch_add(fetched, Ordering::Relaxed);
        self.resumed.fetch_add(resumed, Ordering::Relaxed);
    }

    pub fn fetched(&self) -> u64 {
        self.fetched.load(Ordering::Relaxed)
    }

    pub fn resumed(&self) -> u64 {
        self.resumed.load(Ordering::Relaxed)
    }
}
