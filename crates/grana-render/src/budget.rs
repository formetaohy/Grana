use grana_gpu::Memory;

const FALLBACK: u64 = 512 << 20;

const FLOOR: u64 = 256 << 20;

const CEILING: u64 = 4 << 30;

const DISCRETE_SHARE: u64 = 3;

const INTEGRATED_SHARE: u64 = 8;

pub fn texture_budget(memory: Option<&Memory>) -> u64 {
    let Some(memory) = memory else {
        return FALLBACK;
    };
    let pool = memory.pool();
    let (pool, share) = if memory.integrated() {
        (
            pool.local().saturating_add(pool.non_local()),
            INTEGRATED_SHARE,
        )
    } else {
        (pool.local(), DISCRETE_SHARE)
    };
    (pool / share).clamp(FLOOR, CEILING)
}
