@group(1) @binding(0) var<storage, read> bricks: array<BrickRecord>;
@group(2) @binding(0) var atlases: binding_array<texture_3d<f32>, 16>;
@group(2) @binding(1) var atlas_sampler: sampler;

const GRADIENT_EPSILON: f32 = 1.0e-8;

fn unproject(ndc: vec3<f32>) -> vec3<f32> {
    let homogeneous = frame.inv_view_projection * vec4<f32>(ndc, 1.0);
    return homogeneous.xyz / homogeneous.w;
}

fn sample_volume(channel: u32, uvw: vec3<f32>) -> f32 {
    let cell = brick_cell(frame.grid.xyz, uvw);
    let record = bricks[brick_index(frame.grid.xyz, cell)];
    if (record.slot == 0u) {
        return 0.0;
    }
    let texel = atlas_texel(
        frame.grid.xyz,
        frame.atlas.xyz,
        frame.dims.xyz,
        record.slot - 1u,
        uvw,
    );
    return textureSampleLevel(atlases[channel], atlas_sampler, texel, 0.0).r;
}

fn channel_alpha(uvw: vec3<f32>) -> f32 {
    let count = u32(frame.march.z);
    var per_voxel = 0.0;
    for (var channel = 0u; channel < count; channel += 1u) {
        if (channel_opacity(channel) <= 0.0) {
            continue;
        }
        let alpha = clamp(channel_sample(channel, sample_volume(channel, uvw)).a, 0.0, 0.999);
        per_voxel = 1.0 - (1.0 - per_voxel) * (1.0 - alpha);
    }
    return per_voxel;
}

fn ray_uvw(t: f32, direction: vec3<f32>) -> vec3<f32> {
    return (frame.eye.xyz + direction * t - frame.box_min.xyz) / frame.box_size.xyz;
}

fn boundary_delta(uvw: vec3<f32>, direction: vec3<f32>, minimum: f32) -> f32 {
    let grid = vec3<f32>(f32(frame.grid.x), f32(frame.grid.y), f32(frame.grid.z));
    var delta = 1.0e20;
    for (var axis = 0u; axis < 3u; axis += 1u) {
        let step = direction[axis];
        if (abs(step) > 1.0e-9) {
            let cell = uvw[axis] * grid[axis];
            let next = select(floor(cell) + 1.0, ceil(cell) - 1.0, step < 0.0);
            delta = min(delta, (next / grid[axis] - uvw[axis]) / step);
        }
    }
    return max(delta, minimum);
}

fn density_gradient(channel: u32, uvw: vec3<f32>, step: vec3<f32>) -> vec3<f32> {
    let dx = sample_volume(channel, uvw + vec3<f32>(step.x, 0.0, 0.0))
        - sample_volume(channel, uvw - vec3<f32>(step.x, 0.0, 0.0));
    let dy = sample_volume(channel, uvw + vec3<f32>(0.0, step.y, 0.0))
        - sample_volume(channel, uvw - vec3<f32>(0.0, step.y, 0.0));
    let dz = sample_volume(channel, uvw + vec3<f32>(0.0, 0.0, step.z))
        - sample_volume(channel, uvw - vec3<f32>(0.0, 0.0, step.z));
    return vec3<f32>(dx, dy, dz);
}

fn shaded_normal(gradient: vec3<f32>, direction: vec3<f32>) -> vec3<f32> {
    let voxel = frame.box_size.xyz / frame.dims.xyz;
    var normal = -gradient / voxel;
    if (length(normal) < GRADIENT_EPSILON) {
        return vec3<f32>(0.0);
    }
    normal = normalize(normal);
    if (dot(normal, direction) > 0.0) {
        normal = -normal;
    }
    return normal;
}
