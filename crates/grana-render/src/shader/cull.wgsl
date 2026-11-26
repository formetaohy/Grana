@group(1) @binding(0) var<storage, read_write> bricks: array<BrickRecord>;
@group(1) @binding(1) var<uniform> schedule: ScheduleData;
@group(2) @binding(0) var<storage, read_write> plan: array<atomic<u32>>;

fn box_bounds(
    origin: vec3<f32>,
    axes: array<vec3<f32>, 3>,
    low: vec3<f32>,
    high: vec3<f32>,
) -> array<vec3<f32>, 2> {
    var world_low = origin;
    var world_high = origin;
    for (var axis = 0u; axis < 3u; axis += 1u) {
        let first = axes[axis] * low[axis];
        let second = axes[axis] * high[axis];
        world_low += min(first, second);
        world_high += max(first, second);
    }
    return array<vec3<f32>, 2>(world_low, world_high);
}

fn brick_box(cell: vec3<u32>) -> array<vec3<f32>, 2> {
    let dims = frame.dims.xyz;
    let low = vec3<f32>(cell * 32u);
    let high = min(low + vec3<f32>(32.0), dims);
    return box_bounds(
        volume_origin(),
        array<vec3<f32>, 3>(
            volume_axis(0u),
            volume_axis(1u),
            volume_axis(2u),
        ),
        low,
        high,
    );
}

fn volume_bucket(center: vec3<f32>) -> u32 {
    let distance = max(length(center - frame.eye.xyz) * schedule.bucket_scale, 0.0);
    return min(u32(floor(distance)), BUCKETS - 1u);
}

fn volume_cell(index: u32) -> vec3<u32> {
    let grid = frame.grids.xyz;
    return vec3<u32>(
        index % grid.x,
        (index / grid.x) % grid.y,
        index / (grid.x * grid.y),
    );
}

fn emit(bucket: u32, index: u32) {
    let record = bricks[schedule.base + index];
    if (brick_hollow(record)) {
        return;
    }
    let base = atomicLoad(&plan[BASE_BASE + bucket]);
    if (base == EXCLUDED) {
        return;
    }
    let offset = atomicAdd(&plan[FILL_BASE + bucket], 1u);
    var entry = index;
    if (brick_slot(record) == 0u) {
        entry = index | MISSING;
    }
    atomicStore(&plan[DEMAND_BASE + base + offset], entry);
}

@compute @workgroup_size(64)
fn volume_count(@builtin(global_invocation_id) id: vec3<u32>) {
    let grid = frame.grids.xyz;
    let index = id.x;
    if (index >= grid.x * grid.y * grid.z) {
        return;
    }
    if (brick_hollow(bricks[schedule.base + index])) {
        return;
    }
    let bounds = brick_box(volume_cell(index));
    if (outside_frustum(bounds[0], bounds[1])
        || outside_region_bounds(bounds[0], bounds[1])
        || outside_slice(bounds[0], bounds[1]))
    {
        return;
    }
    let bucket = volume_bucket(0.5 * (bounds[0] + bounds[1]));
    atomicAdd(&plan[0], 1u);
    atomicAdd(&plan[VISIBLE_BASE + bucket], 1u);
}

@compute @workgroup_size(64)
fn volume_scatter(@builtin(global_invocation_id) id: vec3<u32>) {
    let grid = frame.grids.xyz;
    let index = id.x;
    if (index >= grid.x * grid.y * grid.z) {
        return;
    }
    let bounds = brick_box(volume_cell(index));
    if (outside_frustum(bounds[0], bounds[1])
        || outside_region_bounds(bounds[0], bounds[1])
        || outside_slice(bounds[0], bounds[1]))
    {
        return;
    }
    emit(volume_bucket(0.5 * (bounds[0] + bounds[1])), index);
}