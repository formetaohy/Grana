use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Revision(u64);

impl Revision {
    pub const ZERO: Self = Self(0);
    pub const FIRST: Self = Self(1);

    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    pub const fn raw(self) -> u64 {
        self.0
    }

    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }

    pub fn mint() -> Self {
        Self(counter().fetch_add(1, Ordering::Relaxed))
    }
}

fn counter() -> &'static AtomicU64 {
    static COUNTER: OnceLock<AtomicU64> = OnceLock::new();
    COUNTER.get_or_init(|| AtomicU64::new(seed()))
}

fn seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(1, |elapsed| elapsed.as_nanos() as u64)
        | 1
}

#[derive(Default)]
pub struct Revisions {
    held: Revision,
}

impl Revisions {
    pub const fn new() -> Self {
        Self {
            held: Revision::ZERO,
        }
    }

    pub fn mint(&mut self) -> Revision {
        self.held = Revision::mint();
        self.held
    }

    pub fn current(&self) -> Revision {
        self.held
    }
}
