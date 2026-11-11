use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Debug, Default)]
pub struct Cancel {
    flag: Arc<AtomicBool>,
}

impl Cancel {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn trigger(&self) {
        self.flag.store(true, Ordering::Relaxed);
    }

    pub fn cancelled(&self) -> bool {
        self.flag.load(Ordering::Relaxed)
    }

    pub fn flag(&self) -> &AtomicBool {
        &self.flag
    }
}
