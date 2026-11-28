const WALK_EPSILON: f32 = 1.0e-6;

struct Sight {
    cell: vec3<u32>,
    slot: u32,
    usable: bool,
    missing: bool,
    voxel: vec3<f32>,
}

struct Walk {
    sight: Sight,
    entry: f32,
    exit: f32,
    voxel: vec3<f32>,
    rate: vec3<f32>,
    base: vec3<f32>,
    far: vec3<f32>,
    tap: BrickTap,
    voxels: f32,
}

fn volume_sight(origin: vec3<f32>, t: f32, direction: vec3<f32>) -> Sight {
    let g = volume_frame();
    let voxel = voxel_at(voxel_ray(origin, direction), t);
    let cell = brick_of(g.grid, g.dims, voxel);
    let record = volume_record(brick_index(g.grid, cell));
    let slot = brick_display_slot(record);
    var usable = false;
    if (slot != 0u) {
        usable = occupied_brick(slot - 1u);
    }
    return Sight(cell, slot, usable, slot == 0u && !brick_hollow(record), voxel);
}

fn inside_volume(sight: Sight) -> bool {
    let dims = frame.dims.xyz;
    return all(sight.voxel >= vec3<f32>(0.0)) && all(sight.voxel <= dims);
}

fn walk_open(direction: vec3<f32>, begin: f32, finish: f32, sight: Sight) -> Walk {
    let g = volume_frame();
    let rate = volume_rate(direction) * g.dims;
    let base = brick_origin(sight.cell);
    let far = min(base + vec3<f32>(BRICK_SIDE), g.dims);
    let exit = box_exit(base, far, sight.voxel, rate);
    var tap = brick_tap(NO_SLOT, base);
    if (sight.usable) {
        tap = brick_tap(sight.slot - 1u, base);
    }
    return Walk(
        sight,
        begin,
        min(begin + max(exit, max(begin, 1.0) * WALK_EPSILON), finish),
        sight.voxel,
        rate,
        base,
        far,
        tap,
        length(rate),
    );
}

fn walk_voxel(walk: Walk, t: f32) -> vec3<f32> {
    return walk.voxel + walk.rate * (t - walk.entry);
}

fn walk_cell(walk: Walk, voxel: vec3<f32>) -> vec3<u32> {
    return cell_coord(voxel - walk.base);
}

fn walk_cell_span(walk: Walk, coord: vec3<u32>, voxel: vec3<f32>) -> f32 {
    return cell_span(walk.base, coord, walk.far, voxel, walk.rate);
}

fn walk_local(walk: Walk, voxel: vec3<f32>) -> vec3<f32> {
    return tap_local(walk.tap, voxel);
}

fn walk_stride(cursor: f32) -> f32 {
    return local_step(cursor);
}