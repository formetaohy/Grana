const BRICK_SIDE: f32 = 32.0;
const PADDED_SIDE: f32 = 34.0;
const HOLLOW: u32 = 2u;

struct BrickRecord {
    slot: u32,
    flags: u32,
    time: u32,
    reserved: u32,
    maxima: array<vec4<f32>, 4>,
}

fn brick_holds(record: BrickRecord) -> bool {
    return record.time == frame.tag.x;
}

fn brick_hollow(record: BrickRecord) -> bool {
    return brick_holds(record) && record.slot == 0u && (record.flags & HOLLOW) != 0u;
}

fn brick_slot(record: BrickRecord) -> u32 {
    return select(0u, record.slot, brick_holds(record));
}

fn brick_display_slot(record: BrickRecord) -> u32 {
    if (frame.tag.y != 0u) {
        return record.slot;
    }
    return brick_slot(record);
}

fn brick_cell(grid: vec3<u32>, dims: vec3<f32>, uvw: vec3<f32>) -> vec3<u32> {
    let voxel = clamp(uvw, vec3<f32>(0.0), vec3<f32>(1.0)) * dims;
    let cell = floor(voxel / BRICK_SIDE);
    let last = vec3<f32>(grid) - vec3<f32>(1.0);
    return vec3<u32>(clamp(cell, vec3<f32>(0.0), last));
}

fn brick_origin(cell: vec3<u32>) -> vec3<f32> {
    return vec3<f32>(cell) * BRICK_SIDE;
}

fn brick_edge(uvw: vec3<f32>, dims: vec3<f32>, direction: vec3<f32>) -> vec3<f32> {
    var edge = vec3<f32>(0.0);
    for (var axis = 0u; axis < 3u; axis += 1u) {
        let dim = dims[axis];
        let voxel = clamp(uvw[axis], 0.0, 1.0) * dim;
        let origin = floor(voxel / BRICK_SIDE) * BRICK_SIDE;
        let next = select(min(origin + BRICK_SIDE, dim), origin, direction[axis] < 0.0);
        edge[axis] = next / dim;
    }
    return edge;
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

fn atlas_texel(
    grid: vec3<u32>,
    atlas_grid: vec3<u32>,
    dims: vec3<f32>,
    slot: u32,
    uvw: vec3<f32>,
) -> vec3<f32> {
    let clamped = clamp(uvw, vec3<f32>(0.0), vec3<f32>(1.0));
    let local = clamped * dims - brick_origin(brick_cell(grid, dims, clamped));
    let size = vec3<f32>(f32(atlas_grid.x), f32(atlas_grid.y), f32(atlas_grid.z)) * PADDED_SIDE;
    return (slot_origin(atlas_grid, slot) + local + vec3<f32>(1.0)) / size;
}
