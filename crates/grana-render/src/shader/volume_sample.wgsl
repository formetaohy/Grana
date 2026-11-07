@group(1) @binding(0) var<storage, read> bricks: array<BrickRecord>;
@group(1) @binding(1) var<storage, read> summaries: array<vec2<u32>>;
@group(2) @binding(0) var atlases: binding_array<texture_3d<f32>, 4>;
@group(2) @binding(1) var atlas_sampler: sampler;

const GRADIENT_EPSILON: f32 = 1.0e-8;
const CELL_SIDE: f32 = 8.0;
const CELL_AXIS: f32 = 4.0;
const CELLS: u32 = 64u;
const PLANES: u32 = 4u;
const UNIT: f32 = 65535.0;

fn cell_maxima(slot: u32, cell: u32, plane: u32) -> vec4<f32> {
    let word = summaries[slot * (CELLS * PLANES) + cell * PLANES + plane];
    return vec4<f32>(
        f32(word.x & 0xffffu) / UNIT,
        f32(word.x >> 16u) / UNIT,
        f32(word.y & 0xffffu) / UNIT,
        f32(word.y >> 16u) / UNIT,
    );
}

fn cell_voxel(uvw: vec3<f32>, brick: vec3<u32>) -> vec3<f32> {
    return clamp(uvw, vec3<f32>(0.0), vec3<f32>(1.0)) * frame.dims.xyz - brick_origin(brick);
}

fn cell_coord(local: vec3<f32>) -> vec3<u32> {
    return vec3<u32>(
        clamp(floor(local / CELL_SIDE), vec3<f32>(0.0), vec3<f32>(CELL_AXIS - 1.0)),
    );
}

fn cell_index(cell: vec3<u32>) -> u32 {
    return cell.x + u32(CELL_AXIS) * (cell.y + u32(CELL_AXIS) * cell.z);
}

fn cell_edge(uvw: vec3<f32>, brick: vec3<u32>, direction: vec3<f32>) -> vec3<f32> {
    let dims = frame.dims.xyz;
    let origin = brick_origin(brick);
    let base = floor(cell_voxel(uvw, brick) / CELL_SIDE) * CELL_SIDE;
    var edge = vec3<f32>(0.0);
    for (var axis = 0u; axis < 3u; axis += 1u) {
        let limit = clamp(dims[axis] - origin[axis] - base[axis], 0.0, CELL_SIDE);
        let far = select(base[axis] + limit, base[axis], direction[axis] < 0.0);
        edge[axis] = (origin[axis] + far) / dims[axis];
    }
    return edge;
}

fn cell_delta(uvw: vec3<f32>, brick: vec3<u32>, direction: vec3<f32>, minimum: f32) -> f32 {
    let edge = cell_edge(uvw, brick, direction);
    var delta = 1.0e20;
    for (var axis = 0u; axis < 3u; axis += 1u) {
        let step = direction[axis];
        if (abs(step) > 1.0e-9) {
            delta = min(delta, (edge[axis] - uvw[axis]) / step);
        }
    }
    return max(delta, minimum);
}

fn unproject(ndc: vec3<f32>) -> vec3<f32> {
    let homogeneous = frame.inv_view_projection * vec4<f32>(ndc, 1.0);
    return homogeneous.xyz / homogeneous.w;
}

fn sample_atlas(pack: u32, texel: vec3<f32>) -> vec4<f32> {
    return textureSampleLevel(atlases[pack], atlas_sampler, texel, 0.0);
}

fn sample_pack(pack: u32, uvw: vec3<f32>) -> vec4<f32> {
    let cell = brick_cell(frame.grid.xyz, frame.dims.xyz, uvw);
    let record = bricks[brick_index(frame.grid.xyz, cell)];
    if (record.slot == 0u) {
        return vec4<f32>(0.0);
    }
    let texel = atlas_texel(
        frame.grid.xyz,
        frame.atlas.xyz,
        frame.dims.xyz,
        record.slot - 1u,
        uvw,
    );
    return textureSampleLevel(atlases[pack], atlas_sampler, texel, 0.0);
}

fn sample_volume(channel: u32, uvw: vec3<f32>) -> f32 {
    let values = sample_pack(channel / PACK, uvw);
    return values[channel % PACK];
}

fn channel_alpha(uvw: vec3<f32>) -> f32 {
    let count = u32(frame.march.z);
    let packs = (count + PACK - 1u) / PACK;
    var per_voxel = 0.0;
    for (var pack = 0u; pack < packs; pack += 1u) {
        if (!pack_active(pack, count)) {
            continue;
        }
        let values = sample_pack(pack, uvw);
        for (var component = 0u; component < PACK; component += 1u) {
            let channel = pack * PACK + component;
            if (channel >= count) {
                break;
            }
            if (channel_opacity(channel) <= 0.0) {
                continue;
            }
            let alpha = clamp(channel_sample(channel, values[component]).a, 0.0, 0.999);
            per_voxel = 1.0 - (1.0 - per_voxel) * (1.0 - alpha);
        }
    }
    return per_voxel;
}

fn channel_extinction(uvw: vec3<f32>) -> f32 {
    let count = u32(frame.march.z);
    let packs = (count + PACK - 1u) / PACK;
    var extinction = 0.0;
    for (var pack = 0u; pack < packs; pack += 1u) {
        if (!pack_active(pack, count)) {
            continue;
        }
        let values = sample_pack(pack, uvw);
        for (var component = 0u; component < PACK; component += 1u) {
            let channel = pack * PACK + component;
            if (channel >= count) {
                break;
            }
            let opacity = channel_opacity(channel);
            if (opacity <= 0.0) {
                continue;
            }
            let alpha = clamp(channel_sample(channel, values[component]).a, 0.0, 0.998);
            extinction += -log(1.0 - alpha) * opacity;
        }
    }
    return extinction;
}

fn ray_uvw(t: f32, direction: vec3<f32>) -> vec3<f32> {
    return (frame.eye.xyz + direction * t - frame.box_min.xyz) / frame.box_size.xyz;
}

fn boundary_delta(uvw: vec3<f32>, direction: vec3<f32>, minimum: f32) -> f32 {
    let edge = brick_edge(uvw, frame.dims.xyz, direction);
    var delta = 1.0e20;
    for (var axis = 0u; axis < 3u; axis += 1u) {
        let step = direction[axis];
        if (abs(step) > 1.0e-9) {
            delta = min(delta, (edge[axis] - uvw[axis]) / step);
        }
    }
    return max(delta, minimum);
}

fn pack_gradient(pack: u32, uvw: vec3<f32>, step: vec3<f32>) -> array<vec3<f32>, 4> {
    let dx = sample_pack(pack, uvw + vec3<f32>(step.x, 0.0, 0.0))
        - sample_pack(pack, uvw - vec3<f32>(step.x, 0.0, 0.0));
    let dy = sample_pack(pack, uvw + vec3<f32>(0.0, step.y, 0.0))
        - sample_pack(pack, uvw - vec3<f32>(0.0, step.y, 0.0));
    let dz = sample_pack(pack, uvw + vec3<f32>(0.0, 0.0, step.z))
        - sample_pack(pack, uvw - vec3<f32>(0.0, 0.0, step.z));
    return array<vec3<f32>, 4>(
        vec3<f32>(dx.x, dy.x, dz.x),
        vec3<f32>(dx.y, dy.y, dz.y),
        vec3<f32>(dx.z, dy.z, dz.z),
        vec3<f32>(dx.w, dy.w, dz.w),
    );
}

fn shaded_normal(gradient: vec3<f32>) -> vec3<f32> {
    let voxel = frame.box_size.xyz / frame.dims.xyz;
    var normal = -gradient / voxel;
    if (length(normal) < GRADIENT_EPSILON) {
        return vec3<f32>(0.0);
    }
    normal = normalize(normal);
    if (dot(normal, frame.light.xyz) < 0.0) {
        normal = -normal;
    }
    return normal;
}
