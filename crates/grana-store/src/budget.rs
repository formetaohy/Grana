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
    Brick,
    Decode,
    Region,
    Page,
    Layout,
    Readout,
    Remote,
}

impl Pool {
    pub const COUNT: usize = 7;

    pub const ALL: [Self; Self::COUNT] = [
        Self::Brick,
        Self::Decode,
        Self::Region,
        Self::Page,
        Self::Layout,
        Self::Readout,
        Self::Remote,
    ];

    pub fn all() -> &'static [Self] {
        &Self::ALL
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Brick => "brick",
            Self::Decode => "decode",
            Self::Region => "region",
            Self::Page => "page",
            Self::Layout => "layout",
            Self::Readout => "readout",
            Self::Remote => "remote",
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
            Self::Brick => 0,
            Self::Decode => 1,
            Self::Region => 2,
            Self::Page => 3,
            Self::Layout => 4,
            Self::Readout => 5,
            Self::Remote => 6,
        }
    }

    fn parts(self) -> u64 {
        match self {
            Self::Brick => 16,
            Self::Decode => 16,
            Self::Region => 8,
            Self::Page => 1,
            Self::Layout => 2,
            Self::Readout => 8,
            Self::Remote => 4,
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
