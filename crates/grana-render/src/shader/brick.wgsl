const BRICK_SIDE: f32 = 32.0;
const PADDED_SIDE: f32 = 34.0;
const PADDED_TEXEL: u32 = 34u;
const STRUCTURE_STRIDE: u32 = 2u;
const STRUCTURE_PAD: u32 = 1u;
const STRUCTURE_PHASE: f32 = 2.5;
const HOLLOW: u32 = 2u;
const NO_SLOT: u32 = 0xffffffffu;

struct BrickRecord {
    slot: u32,
    flags: u32,
    time: u32,
    content: u32,
}

struct BrickTap {
    slot: u32,
    base: vec3<f32>,
}

fn brick_holds(record: BrickRecord) -> bool {
    return record.time == frame.tag.x && record.content == frame.tag.w;
}

fn brick_hollow(record: BrickRecord) -> bool {
    return brick_holds(record) && record.slot == 0u && (record.flags & HOLLOW) != 0u;
}

fn brick_slot(record: BrickRecord) -> u32 {
    return select(0u, record.slot, brick_holds(record));
}

fn brick_display_slot(record: BrickRecord) -> u32 {
    if (frame.tag.y != 0u && record.content == frame.tag.w) {
        return record.slot;
    }
    return brick_slot(record);
}

fn brick_of(grid: vec3<u32>, dims: vec3<f32>, voxel: vec3<f32>) -> vec3<u32> {
    let cell = floor(clamp(voxel, vec3<f32>(0.0), dims) / BRICK_SIDE);
    let last = vec3<f32>(grid) - vec3<f32>(1.0);
    return vec3<u32>(clamp(cell, vec3<f32>(0.0), last));
}

fn brick_origin(cell: vec3<u32>) -> vec3<f32> {
    return vec3<f32>(cell) * BRICK_SIDE;
}

fn brick_index(grid: vec3<u32>, cell: vec3<u32>) -> u32 {
    return cell.x + grid.x * (cell.y + grid.y * cell.z);
}

fn slot_cell(slot: u32) -> vec3<u32> {
    let layer = slot / (frame.atlas.x * frame.atlas.y);
    let remainder = slot % (frame.atlas.x * frame.atlas.y);
    return vec3<u32>(remainder % frame.atlas.x, remainder / frame.atlas.x, layer);
}

fn brick_tap(slot: u32, base: vec3<f32>) -> BrickTap {
    return BrickTap(slot, base);
}

fn tap_atlas_texel(tap: BrickTap) -> vec3<u32> {
    return slot_cell(tap.slot) * PADDED_TEXEL + vec3<u32>(1u);
}

fn tap_local(tap: BrickTap, voxel: vec3<f32>) -> vec3<f32> {
    return clamp(voxel, tap.base, tap.base + vec3<f32>(BRICK_SIDE)) - tap.base;
}

fn structure_position(local: vec3<f32>) -> vec3<f32> {
    return (local + vec3<f32>(STRUCTURE_PHASE)) / vec3<f32>(f32(STRUCTURE_STRIDE));
}
