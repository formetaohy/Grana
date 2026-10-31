struct ScheduleData {
    time: u32,
    level: u32,
    capacity: u32,
    layer: u32,
    u0: u32,
    v0: u32,
    u_count: u32,
    v_count: u32,
}

const REQUEST_CAPACITY: u32 = 4096u;

@group(1) @binding(0) var<storage, read_write> bricks: array<BrickRecord>;
@group(1) @binding(1) var<uniform> schedule: ScheduleData;
@group(2) @binding(0) var<storage, read_write> requests: array<atomic<u32>>;

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

fn request(index: u32) {
    atomicAdd(&requests[0], 1u);
    if (bricks[index].slot == 0u) {
        atomicAdd(&requests[2], 1u);
        let slot = atomicAdd(&requests[3], 1u);
        if (slot < schedule.capacity) {
            atomicStore(&requests[4u + slot], index);
        }
    } else {
        let slot = atomicAdd(&requests[1], 1u);
        if (slot < schedule.capacity) {
            atomicStore(&requests[4u + REQUEST_CAPACITY + slot], index);
        }
        bricks[index].flags = brick_opacity(index);
    }
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

@compute @workgroup_size(64)
fn volume_cull(@builtin(global_invocation_id) id: vec3<u32>) {
    let grid = vec3<u32>(frame.grid.xyz);
    let count = grid.x * grid.y * grid.z;
    let index = id.x;
    if (index >= count) {
        return;
    }
    let cell = vec3<u32>(
        index % grid.x,
        (index / grid.x) % grid.y,
        index / (grid.x * grid.y),
    );
    let bounds = brick_box(cell);
    if (outside_frustum(bounds[0], bounds[1]) || outside_region_bounds(bounds[0], bounds[1])) {
        return;
    }
    request(index);
}

@compute @workgroup_size(64)
fn section_cull(@builtin(global_invocation_id) id: vec3<u32>) {
    let total = schedule.u_count * schedule.v_count;
    let index = id.x;
    if (index >= total) {
        return;
    }
    var cell = vec3<u32>(0u);
    cell[section.mapping.x] = schedule.u0 + index % schedule.u_count;
    cell[section.mapping.y] = schedule.v0 + index / schedule.u_count;
    cell[section.mapping.z] = schedule.layer;
    let grid = vec3<u32>(section.grid.xyz);
    if (any(cell >= grid)) {
        return;
    }
    let bounds = section_brick_box(cell);
    if (outside_region_bounds(bounds[0], bounds[1])) {
        return;
    }
    let brick = cell.x + grid.x * (cell.y + grid.y * cell.z);
    request(brick);
}
