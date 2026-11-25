use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

const DEFAULT_TOTAL: u64 = 16 << 30;

const PARTS: u64 = 256;

pub struct Budget {
    total: AtomicU64,
    configured: AtomicBool,
}

static LEDGER: Budget = Budget {
    total: AtomicU64::new(DEFAULT_TOTAL),
    configured: AtomicBool::new(false),
};

static METERS: [Meter; Pool::COUNT] = [const { Meter::new() }; Pool::COUNT];

impl Budget {
    pub fn shared() -> &'static Self {
        &LEDGER
    }

    pub fn set_total(&self, bytes: u64) {
        assert!(bytes > 0, "a cache ledger holds at least one byte");
        assert!(
            !self.configured.swap(true, Ordering::Relaxed),
            "the cache ledger total is set once"
        );
        self.total.store(bytes, Ordering::Relaxed);
    }

    pub fn total(&self) -> u64 {
        self.total.load(Ordering::Relaxed)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Pool {
    Plane,
    Decode,
    Remote,
    Readout,
    Page,
    Layout,
}

impl Pool {
    pub const COUNT: usize = 6;

    pub const ALL: [Self; Self::COUNT] = [
        Self::Plane,
        Self::Decode,
        Self::Remote,
        Self::Readout,
        Self::Page,
        Self::Layout,
    ];

    pub fn all() -> &'static [Self] {
        &Self::ALL
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Plane => "plane",
            Self::Decode => "decode",
            Self::Remote => "remote",
            Self::Readout => "readout",
            Self::Page => "page",
            Self::Layout => "layout",
        }
    }

    pub fn share(self) -> u64 {
        Budget::shared().total().saturating_mul(self.parts()) / PARTS
    }

    pub fn meter(self) -> &'static Meter {
        &METERS[self.index()]
    }

    fn index(self) -> usize {
        match self {
            Self::Plane => 0,
            Self::Decode => 1,
            Self::Remote => 2,
            Self::Readout => 3,
            Self::Page => 4,
            Self::Layout => 5,
        }
    }

    fn parts(self) -> u64 {
        match self {
            Self::Plane => 96,
            Self::Decode => 40,
            Self::Remote => 32,
            Self::Readout => 32,
            Self::Page => 2,
            Self::Layout => 2,
        }
    }
}

pub struct Meter {
    hits: AtomicU64,
    misses: AtomicU64,
    evicted: AtomicU64,
}

impl Meter {
    const fn new() -> Self {
        Self {
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            evicted: AtomicU64::new(0),
        }
    }

    pub fn hit(&self) {
        self.hits.fetch_add(1, Ordering::Relaxed);
    }

    pub fn miss(&self) {
        self.misses.fetch_add(1, Ordering::Relaxed);
    }

    pub fn evict(&self, bytes: u64) {
        if bytes > 0 {
            self.evicted.fetch_add(bytes, Ordering::Relaxed);
        }
    }

    pub fn hits(&self) -> u64 {
        self.hits.load(Ordering::Relaxed)
    }

    pub fn misses(&self) -> u64 {
        self.misses.load(Ordering::Relaxed)
    }

    pub fn evicted(&self) -> u64 {
        self.evicted.load(Ordering::Relaxed)
    }

    pub fn hit_rate(&self) -> f32 {
        let asked = self.hits() + self.misses();
        if asked == 0 {
            0.0
        } else {
            self.hits() as f32 / asked as f32
        }
    }
}
