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
}
