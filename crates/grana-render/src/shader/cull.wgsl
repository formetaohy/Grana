@group(1) @binding(0) var<storage, read_write> bricks: array<BrickRecord>;
@group(1) @binding(1) var<uniform> schedule: ScheduleData;
@group(2) @binding(0) var<storage, read_write> plan: array<atomic<u32>>;

fn brick_opacity(at: u32) -> u32 {
    let channels_count = u32(frame.march.z);
    for (var channel = 0u; channel < channels_count; channel += 1u) {
        if (channels[channel].opacity <= 0.0) {
            continue;
        }
        let maximum = bricks[at].maxima[channel / 4u][channel % 4u];
        if (channel_sample(channel, maximum).a > 0.004) {
            return 1u;
        }
    }
    return 0u;
}

fn level_dims() -> vec3<f32> {
    return frame.dims[schedule.band].xyz;
}

fn level_grid() -> vec3<u32> {
    return frame.grids[schedule.band].xyz;
}

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
    let dims = vec3<f32>(vec3<u32>(level_dims()));
    let low = vec3<f32>(cell * 32u);
    let high = min(low + vec3<f32>(32.0), dims);
    return box_bounds(
        level_origin(schedule.band),
        array<vec3<f32>, 3>(
            level_axis(schedule.band, 0u),
            level_axis(schedule.band, 1u),
            level_axis(schedule.band, 2u),
        ),
        low,
        high,
    );
}

fn section_brick_box(cell: vec3<u32>) -> array<vec3<f32>, 2> {
    let dims = vec3<f32>(vec3<u32>(section.dims.xyz));
    let low = vec3<f32>(cell * 32u);
    let high = min(low + vec3<f32>(32.0), dims);
    return box_bounds(
        section.origin.xyz,
        array<vec3<f32>, 3>(section.axes[0].xyz, section.axes[1].xyz, section.axes[2].xyz),
        low,
        high,
    );
}

fn reach(low: vec3<f32>, high: vec3<f32>) -> vec2<f32> {
    let eye = frame.eye.xyz;
    let near = length(clamp(eye, low, high) - eye);
    let far = length(max(abs(eye - low), abs(eye - high)));
    return vec2<f32>(near, far);
}

fn band_holds(low: vec3<f32>, high: vec3<f32>) -> bool {
    let span = reach(low, high);
    return span.y >= band_lower(schedule.band) && span.x < band_upper(schedule.band);
}

fn volume_bucket(center: vec3<f32>) -> u32 {
    let distance = max(length(center - frame.eye.xyz) * schedule.bucket_scale, 0.0);
    return min(u32(floor(distance)), BUCKETS - 1u);
}

fn section_bucket(center: vec3<f32>) -> u32 {
    let u = abs(section_across(center)) / max(section.right.w, 1.0e-6);
    let v = abs(section_down(center)) / max(section.up.w, 1.0e-6);
    return min(u32(floor(max(u, v) * f32(BUCKETS))), BUCKETS - 1u);
}

fn section_overlap(low: vec3<f32>, high: vec3<f32>) -> bool {
    var dmin = 0.0;
    var dmax = 0.0;
    var smin = 0.0;
    var smax = 0.0;
    var tmin = 0.0;
    var tmax = 0.0;
    for (var axis = 0u; axis < 3u; axis += 1u) {
        let normal = section.plane[axis];
        let near = normal * low[axis];
        let far = normal * high[axis];
        dmin += min(near, far);
        dmax += max(near, far);
        let across = section.right[axis];
        let across_near = across * low[axis];
        let across_far = across * high[axis];
        smin += min(across_near, across_far);
        smax += max(across_near, across_far);
        let down = section.up[axis];
        let down_near = down * low[axis];
        let down_far = down * high[axis];
        tmin += min(down_near, down_far);
        tmax += max(down_near, down_far);
    }
    if (section.plane.w < dmin || section.plane.w > dmax) {
        return false;
    }
    if (smax < section.center.x - section.right.w || smin > section.center.x + section.right.w) {
        return false;
    }
    if (tmax < section.center.y - section.up.w || tmin > section.center.y + section.up.w) {
        return false;
    }
    return true;
}

fn volume_cell(index: u32) -> vec3<u32> {
    let grid = level_grid();
    return vec3<u32>(
        index % grid.x,
        (index / grid.x) % grid.y,
        index / (grid.x * grid.y),
    );
}

fn section_cell(index: u32) -> vec3<u32> {
    return vec3<u32>(
        schedule.x0 + index % schedule.x_count,
        schedule.y0 + (index / schedule.x_count) % schedule.y_count,
        schedule.z0 + index / (schedule.x_count * schedule.y_count),
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
    let grid = level_grid();
    let index = id.x;
    if (index >= grid.x * grid.y * grid.z) {
        return;
    }
    if (brick_hollow(bricks[schedule.base + index])) {
        return;
    }
    let bounds = brick_box(volume_cell(index));
    if (outside_frustum(bounds[0], bounds[1]) || outside_region_bounds(bounds[0], bounds[1])) {
        return;
    }
    if (!band_holds(bounds[0], bounds[1])) {
        return;
    }
    let bucket = volume_bucket(0.5 * (bounds[0] + bounds[1]));
    atomicAdd(&plan[0], 1u);
    atomicAdd(&plan[VISIBLE_BASE + bucket], 1u);
    if (brick_slot(bricks[schedule.base + index]) != 0u) {
        bricks[schedule.base + index].flags = brick_opacity(schedule.base + index);
    }
}

@compute @workgroup_size(64)
fn volume_scatter(@builtin(global_invocation_id) id: vec3<u32>) {
    let grid = level_grid();
    let index = id.x;
    if (index >= grid.x * grid.y * grid.z) {
        return;
    }
    let bounds = brick_box(volume_cell(index));
    if (outside_frustum(bounds[0], bounds[1]) || outside_region_bounds(bounds[0], bounds[1])) {
        return;
    }
    if (!band_holds(bounds[0], bounds[1])) {
        return;
    }
    emit(volume_bucket(0.5 * (bounds[0] + bounds[1])), index);
}

@compute @workgroup_size(64)
fn section_count(@builtin(global_invocation_id) id: vec3<u32>) {
    let total = schedule.x_count * schedule.y_count * schedule.z_count;
    let index = id.x;
    if (index >= total) {
        return;
    }
    let cell = section_cell(index);
    let grid = vec3<u32>(section.grid.xyz);
    if (any(cell >= grid)) {
        return;
    }
    let bounds = section_brick_box(cell);
    if (outside_region_bounds(bounds[0], bounds[1]) || !section_overlap(bounds[0], bounds[1])) {
        return;
    }
    let brick = schedule.base + cell.x + grid.x * (cell.y + grid.y * cell.z);
    if (brick_hollow(bricks[brick])) {
        return;
    }
    let bucket = section_bucket(0.5 * (bounds[0] + bounds[1]));
    atomicAdd(&plan[0], 1u);
    atomicAdd(&plan[VISIBLE_BASE + bucket], 1u);
    if (brick_slot(bricks[brick]) != 0u) {
        bricks[brick].flags = brick_opacity(brick);
    }
}

@compute @workgroup_size(64)
fn section_scatter(@builtin(global_invocation_id) id: vec3<u32>) {
    let total = schedule.x_count * schedule.y_count * schedule.z_count;
    let index = id.x;
    if (index >= total) {
        return;
    }
    let cell = section_cell(index);
    let grid = vec3<u32>(section.grid.xyz);
    if (any(cell >= grid)) {
        return;
    }
    let bounds = section_brick_box(cell);
    if (outside_region_bounds(bounds[0], bounds[1]) || !section_overlap(bounds[0], bounds[1])) {
        return;
    }
    let brick = cell.x + grid.x * (cell.y + grid.y * cell.z);
    emit(section_bucket(0.5 * (bounds[0] + bounds[1])), brick);
}
