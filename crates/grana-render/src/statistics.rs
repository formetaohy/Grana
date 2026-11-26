#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Statistics {
    pub plans: u64,
    pub waits: u64,
    pub uploads: u64,
    pub evictions: u64,
    pub copies: u64,
    pub staged: u64,
    pub payload: u64,
    pub prefetched: u64,
    pub prefetch_hits: u64,
    pub frames: u64,
    pub splats: u64,
    pub splat_instances: u64,
    pub residency: u64,
    pub atlas: u64,
    pub records: u64,
    pub surfaces: u64,
    pub unavailable: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Transfer {
    pub(super) copies: u64,
    pub(super) staged: u64,
    pub(super) payload: u64,
}

impl Transfer {
    pub(super) fn add(&mut self, counters: crate::upload::Counters) {
        self.copies += counters.copies;
        self.staged += counters.staged;
        self.payload += counters.payload;
    }
}
