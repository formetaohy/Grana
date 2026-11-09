struct ScheduleData {
    time: u32,
    band: u32,
    capacity: u32,
    x0: u32,
    y0: u32,
    z0: u32,
    x_count: u32,
    y_count: u32,
    bucket_scale: f32,
    base: u32,
    z_count: u32,
    reserved: u32,
}

const BUCKETS: u32 = 128u;
const VISIBLE_BASE: u32 = 4u;
const BASE_BASE: u32 = VISIBLE_BASE + BUCKETS;
const FILL_BASE: u32 = BASE_BASE + BUCKETS;
const DEMAND_BASE: u32 = FILL_BASE + BUCKETS;
const MISSING: u32 = 0x80000000u;
const EXCLUDED: u32 = 0xffffffffu;
