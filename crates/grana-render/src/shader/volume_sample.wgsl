@group(1) @binding(0) var<storage, read> bricks: array<BrickRecord>;
@group(2) @binding(0) var atlases: binding_array<texture_3d<f32>, 4>;
@group(2) @binding(1) var atlas_sampler: sampler;

const GRADIENT_EPSILON: f32 = 1.0e-8;

fn unproject(ndc: vec3<f32>) -> vec3<f32> {
    let homogeneous = frame.inv_view_projection * vec4<f32>(ndc, 1.0);
    return homogeneous.xyz / homogeneous.w;
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
