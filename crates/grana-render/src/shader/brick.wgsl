const BRICK_SIDE: f32 = 32.0;
const PADDED_SIDE: f32 = 34.0;
const HOLLOW: u32 = 2u;

struct BrickRecord {
    slot: u32,
    flags: u32,
    time: u32,
    content: u32,
}

struct TexelMap {
    offset: vec3<f32>,
    scale: vec3<f32>,
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

fn slot_origin(atlas_grid: vec3<u32>, slot: u32) -> vec3<f32> {
    let layer = slot / (atlas_grid.x * atlas_grid.y);
    let remainder = slot % (atlas_grid.x * atlas_grid.y);
    let row = remainder / atlas_grid.x;
    let column = remainder % atlas_grid.x;
    return vec3<f32>(f32(column), f32(row), f32(layer)) * PADDED_SIDE;
}

fn texel_map(slot: u32, base: vec3<f32>) -> TexelMap {
    let extent = vec3<f32>(frame.atlas.xyz) * PADDED_SIDE;
    return TexelMap(
        slot_origin(frame.atlas.xyz, slot) + vec3<f32>(1.0) - base,
        vec3<f32>(1.0) / extent,
    );
}

fn texel_at(map: TexelMap, voxel: vec3<f32>) -> vec3<f32> {
    return (voxel + map.offset) * map.scale;
}
