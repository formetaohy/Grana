struct ScheduleData {
    time: u32,
    level: u32,
    capacity: u32,
    layer: u32,
    u0: u32,
    v0: u32,
    u_count: u32,
    v_count: u32,
    bucket_scale: f32,
    reserved0: u32,
    reserved1: u32,
    reserved2: u32,
}

const BUCKETS: u32 = 128u;
const VISIBLE_BASE: u32 = 4u;
const BASE_BASE: u32 = VISIBLE_BASE + BUCKETS;
const FILL_BASE: u32 = BASE_BASE + BUCKETS;
const DEMAND_BASE: u32 = FILL_BASE + BUCKETS;
const MISSING: u32 = 0x80000000u;
const EXCLUDED: u32 = 0xffffffffu;
