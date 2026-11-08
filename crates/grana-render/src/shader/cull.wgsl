@group(1) @binding(0) var<storage, read_write> bricks: array<BrickRecord>;
@group(1) @binding(1) var<uniform> schedule: ScheduleData;
@group(2) @binding(0) var<storage, read_write> plan: array<atomic<u32>>;

fn brick_opacity(index: u32) -> u32 {
    let channels_count = u32(frame.march.z);
    for (var channel = 0u; channel < channels_count; channel += 1u) {
        if (channels[channel].opacity <= 0.0) {
            continue;
        }
        let maximum = bricks[index].maxima[channel / 4u][channel % 4u];
        if (channel_sample(channel, maximum).a > 0.004) {
            return 1u;
        }
    }
    return 0u;
}

fn brick_box(cell: vec3<u32>) -> array<vec3<f32>, 2> {
    let dims = vec3<u32>(frame.dims.xyz);
    let origin = cell * 32u;
    let size = min(vec3<u32>(32u), dims - origin);
    let voxel = frame.box_size.xyz / frame.dims.xyz;
    let low = frame.box_min.xyz + vec3<f32>(origin) * voxel;
    let high = low + vec3<f32>(size) * voxel;
    return array<vec3<f32>, 2>(low, high);
}

fn section_brick_box(cell: vec3<u32>) -> array<vec3<f32>, 2> {
    let dims = vec3<u32>(section.dims.xyz);
    let origin = cell * 32u;
    let size = min(vec3<u32>(32u), dims - origin);
    let voxel = section.world.xyz / section.dims.xyz;
    let low = section.box_min.xyz + vec3<f32>(origin) * voxel;
    let high = low + vec3<f32>(size) * voxel;
    return array<vec3<f32>, 2>(low, high);
}

fn volume_bucket(center: vec3<f32>) -> u32 {
    let distance = max(length(center - frame.eye.xyz) * schedule.bucket_scale, 0.0);
    return min(u32(floor(distance)), BUCKETS - 1u);
}

fn section_bucket(center: vec3<f32>) -> u32 {
    let uvw = (center - section.box_min.xyz) / section.world.xyz;
    let u = abs(uvw[section.mapping.x] - section.uv_center.x) / max(section.uv_scale.x, 1.0e-6);
    let v = abs(uvw[section.mapping.y] - section.uv_center.y) / max(section.uv_scale.y, 1.0e-6);
    return min(u32(floor(max(u, v) * f32(BUCKETS))), BUCKETS - 1u);
}

fn volume_cell(index: u32) -> vec3<u32> {
    let grid = vec3<u32>(frame.grid.xyz);
    return vec3<u32>(
        index % grid.x,
        (index / grid.x) % grid.y,
        index / (grid.x * grid.y),
    );
}

fn section_cell(index: u32) -> vec3<u32> {
    var cell = vec3<u32>(0u);
    cell[section.mapping.x] = schedule.u0 + index % schedule.u_count;
    cell[section.mapping.y] = schedule.v0 + index / schedule.u_count;
    cell[section.mapping.z] = schedule.layer;
    return cell;
}

fn emit(bucket: u32, index: u32) {
    let record = bricks[index];
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
    let grid = vec3<u32>(frame.grid.xyz);
    let index = id.x;
    if (index >= grid.x * grid.y * grid.z) {
        return;
    }
    if (brick_hollow(bricks[index])) {
        return;
    }
    let bounds = brick_box(volume_cell(index));
    if (outside_frustum(bounds[0], bounds[1]) || outside_region_bounds(bounds[0], bounds[1])) {
        return;
    }
    let bucket = volume_bucket(0.5 * (bounds[0] + bounds[1]));
    atomicAdd(&plan[0], 1u);
    atomicAdd(&plan[VISIBLE_BASE + bucket], 1u);
    if (brick_slot(bricks[index]) != 0u) {
        bricks[index].flags = brick_opacity(index);
    }
}

@compute @workgroup_size(64)
fn volume_scatter(@builtin(global_invocation_id) id: vec3<u32>) {
    let grid = vec3<u32>(frame.grid.xyz);
    let index = id.x;
    if (index >= grid.x * grid.y * grid.z) {
        return;
    }
    let bounds = brick_box(volume_cell(index));
    if (outside_frustum(bounds[0], bounds[1]) || outside_region_bounds(bounds[0], bounds[1])) {
        return;
    }
    emit(volume_bucket(0.5 * (bounds[0] + bounds[1])), index);
}

@compute @workgroup_size(64)
fn section_count(@builtin(global_invocation_id) id: vec3<u32>) {
    let total = schedule.u_count * schedule.v_count;
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
    if (outside_region_bounds(bounds[0], bounds[1])) {
        return;
    }
    let brick = cell.x + grid.x * (cell.y + grid.y * cell.z);
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
    let total = schedule.u_count * schedule.v_count;
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
    if (outside_region_bounds(bounds[0], bounds[1])) {
        return;
    }
    let brick = cell.x + grid.x * (cell.y + grid.y * cell.z);
    emit(section_bucket(0.5 * (bounds[0] + bounds[1])), brick);
}
