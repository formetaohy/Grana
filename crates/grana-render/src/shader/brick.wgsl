const BRICK_SIDE: f32 = 32.0;
const PADDED_SIDE: f32 = 34.0;

struct BrickRecord {
    slot: u32,
    flags: u32,
    reserved: vec2<u32>,
    maxima: array<vec4<f32>, 4>,
}

fn brick_cell(grid: vec3<u32>, uvw: vec3<f32>) -> vec3<u32> {
    let scaled = clamp(uvw, vec3<f32>(0.0), vec3<f32>(1.0))
        * vec3<f32>(f32(grid.x), f32(grid.y), f32(grid.z));
    let rounded = floor(scaled);
    let cell = vec3<u32>(u32(rounded.x), u32(rounded.y), u32(rounded.z));
    return min(cell, grid - vec3<u32>(1u));
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
    let cell = brick_cell(grid, clamped);
    let origin = vec3<f32>(f32(cell.x), f32(cell.y), f32(cell.z)) * BRICK_SIDE;
    let local = clamped * dims - origin;
    let size = vec3<f32>(f32(atlas_grid.x), f32(atlas_grid.y), f32(atlas_grid.z)) * PADDED_SIDE;
    return (slot_origin(atlas_grid, slot) + local + vec3<f32>(1.0)) / size;
}
