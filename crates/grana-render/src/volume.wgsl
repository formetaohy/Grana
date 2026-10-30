struct Uniforms {
    inv_view_projection: mat4x4<f32>,
    eye: vec4<f32>,
    box_min: vec4<f32>,
    box_size: vec4<f32>,
    shading: vec4<f32>,
    march: vec4<f32>,
    light: vec4<f32>,
    grid: vec4<u32>,
    atlas: vec4<u32>,
    dims: vec4<f32>,
    channel_opacity: array<vec4<f32>, 2>,
    channel_window: array<vec4<f32>, 2>,
    channel_inverse: array<vec4<f32>, 2>,
}

@group(0) @binding(0) var<uniform> uniforms: Uniforms;
@group(0) @binding(1) var lut: texture_2d<f32>;
@group(0) @binding(2) var lut_sampler: sampler;
@group(0) @binding(3) var<storage, read> bricks: array<BrickRecord>;
@group(1) @binding(0) var atlases: binding_array<texture_3d<f32>, 8>;
@group(1) @binding(1) var atlas_sampler: sampler;
@group(2) @binding(0) var object_depth: texture_depth_2d;

const MAX_CELL_STEPS: u32 = 4096u;
const MINIMUM_TRANSMITTANCE: f32 = 0.003;
const MINIMUM_BRICK_ALPHA: f32 = 0.004;
const MINIMUM_VOXEL_ALPHA: f32 = 1.0e-4;
const GRADIENT_EPSILON: f32 = 1.0e-8;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) ndc: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    let corners = array<vec2<f32>, 3>(vec2<f32>(-1.0, -1.0), vec2<f32>(3.0, -1.0), vec2<f32>(-1.0, 3.0));
    var output: VertexOutput;
    output.clip_position = vec4<f32>(corners[index], 0.0, 1.0);
    output.ndc = corners[index];
    return output;
}

fn unproject(ndc: vec3<f32>) -> vec3<f32> {
    let homogeneous = uniforms.inv_view_projection * vec4<f32>(ndc, 1.0);
    return homogeneous.xyz / homogeneous.w;
}

fn object_distance(depth: f32, ndc: vec2<f32>) -> f32 {
    if (depth >= 1.0) {
        return 1.0e20;
    }
    return length(unproject(vec3<f32>(ndc, depth)) - uniforms.eye.xyz);
}

fn channel_opacity(index: u32) -> f32 {
    return uniforms.channel_opacity[index / 4u][index % 4u];
}

fn windowed(index: u32, unit: f32) -> f32 {
    return windowed_unit(
        uniforms.channel_window[index / 4u][index % 4u],
        uniforms.channel_inverse[index / 4u][index % 4u],
        unit,
    );
}

fn channel_sample(index: u32, unit: f32) -> vec4<f32> {
    return textureSampleLevel(lut, lut_sampler, vec2<f32>(windowed(index, unit), lut_row(index)), 0.0);
}

fn sample_channel(index: u32, uvw: vec3<f32>) -> f32 {
    let cell = brick_cell(uniforms.grid.xyz, uvw);
    let record = bricks[brick_index(uniforms.grid.xyz, cell)];
    if (record.slot == 0u) {
        return 0.0;
    }
    let texel = atlas_texel(
        uniforms.grid.xyz,
        uniforms.atlas.xyz,
        uniforms.dims.xyz,
        record.slot - 1u,
        uvw,
    );
    return textureSampleLevel(atlases[index], atlas_sampler, texel, 0.0).r;
}

fn ray_uvw(t: f32, direction: vec3<f32>) -> vec3<f32> {
    return (uniforms.eye.xyz + direction * t - uniforms.box_min.xyz) / uniforms.box_size.xyz;
}

fn brick_visible(record: BrickRecord) -> bool {
    let count = u32(uniforms.march.z);
    for (var channel = 0u; channel < count; channel += 1u) {
        if (channel_opacity(channel) <= 0.0) {
            continue;
        }
        let maximum = record.maxima[channel / 4u][channel % 4u];
        if (channel_sample(channel, maximum).a > MINIMUM_BRICK_ALPHA) {
            return true;
        }
    }
    return false;
}

fn boundary_delta(uvw: vec3<f32>, direction: vec3<f32>, minimum: f32) -> f32 {
    let grid = vec3<f32>(f32(uniforms.grid.x), f32(uniforms.grid.y), f32(uniforms.grid.z));
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
    let dx = sample_channel(channel, uvw + vec3<f32>(step.x, 0.0, 0.0))
        - sample_channel(channel, uvw - vec3<f32>(step.x, 0.0, 0.0));
    let dy = sample_channel(channel, uvw + vec3<f32>(0.0, step.y, 0.0))
        - sample_channel(channel, uvw - vec3<f32>(0.0, step.y, 0.0));
    let dz = sample_channel(channel, uvw + vec3<f32>(0.0, 0.0, step.z))
        - sample_channel(channel, uvw - vec3<f32>(0.0, 0.0, step.z));
    return vec3<f32>(dx, dy, dz);
}

fn shaded_normal(gradient: vec3<f32>, direction: vec3<f32>) -> vec3<f32> {
    let voxel = uniforms.box_size.xyz / uniforms.dims.xyz;
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

fn lit(color: vec3<f32>, normal: vec3<f32>, direction: vec3<f32>) -> vec3<f32> {
    if (length(normal) < 0.5) {
        return color;
    }
    let half_vector = normalize(uniforms.light.xyz - direction);
    let diffuse = max(dot(normal, uniforms.light.xyz), 0.0);
    let specular = pow(max(dot(normal, half_vector), 0.0), uniforms.shading.w);
    return color * (uniforms.shading.x + uniforms.shading.y * diffuse)
        + vec3<f32>(uniforms.shading.z * specular);
}

fn shade_volume(channel: u32, color: vec3<f32>, uvw: vec3<f32>, step: vec3<f32>, direction: vec3<f32>) -> vec3<f32> {
    let gradient = density_gradient(channel, uvw, step);
    if (length(gradient) < GRADIENT_EPSILON) {
        return color;
    }
    return lit(color, shaded_normal(gradient, direction), direction);
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let eye = uniforms.eye.xyz;
    let direction = normalize(unproject(vec3<f32>(input.ndc, 1.0)) - eye);
    let box_min = uniforms.box_min.xyz;
    let box_size = uniforms.box_size.xyz;
    let box_max = box_min + box_size;

    let inverse_direction = vec3<f32>(1.0) / direction;
    let near_t = (box_min - eye) * inverse_direction;
    let far_t = (box_max - eye) * inverse_direction;
    let lowest_t = min(near_t, far_t);
    let highest_t = max(near_t, far_t);
    let entry = max(max(lowest_t.x, lowest_t.y), lowest_t.z);
    let object = object_distance(textureLoad(object_depth, vec2<i32>(input.clip_position.xy), 0), input.ndc);
    let exit = min(min(min(highest_t.x, highest_t.y), highest_t.z), object);
    let start = max(entry, 0.0);

    var radiance = vec3<f32>(0.0);
    var transmittance = 1.0;

    if (exit > start) {
        let step_length = uniforms.march.x;
        let max_steps = u32(uniforms.march.w);
        let channel_count = u32(uniforms.march.z);
        let voxel_world = box_size / uniforms.dims.xyz;
        let gradient_step = vec3<f32>(1.0) / uniforms.dims.xyz;
        let direction_uvw = direction / box_size;
        let voxels_per_world = length(direction / voxel_world);

        var t = start;
        var steps = 0u;
        var cells = 0u;
        var complete = false;
        while (t < exit && steps < max_steps && cells < MAX_CELL_STEPS && !complete) {
            cells += 1u;
            let uvw = ray_uvw(t, direction);
            let cell = brick_cell(uniforms.grid.xyz, uvw);
            let record = bricks[brick_index(uniforms.grid.xyz, cell)];
            let boundary = min(t + boundary_delta(uvw, direction_uvw, step_length * 0.25), exit);
            if (record.slot == 0u || !brick_visible(record)) {
                t = boundary;
                continue;
            }
            while (t < boundary && steps < max_steps) {
                let segment = min(step_length, boundary - t);
                let position = ray_uvw(t, direction);
                var optical_depth = 0.0;
                var emission = vec3<f32>(0.0);

                for (var channel = 0u; channel < channel_count; channel += 1u) {
                    let opacity = channel_opacity(channel);
                    if (opacity <= 0.0) {
                        continue;
                    }
                    let unit = sample_channel(channel, position);
                    let sample = channel_sample(channel, unit);
                    let per_voxel = clamp(sample.a, 0.0, 0.999);
                    if (per_voxel > MINIMUM_VOXEL_ALPHA) {
                        let tau = -log(1.0 - per_voxel) * segment * voxels_per_world * opacity;
                        var color = sample.rgb;
                        if (per_voxel > 0.02) {
                            color = shade_volume(channel, color, position, gradient_step, direction);
                        }
                        optical_depth += tau;
                        emission += tau * color;
                    }
                }

                if (optical_depth > 0.0) {
                    let segment_transmittance = exp(-optical_depth);
                    let average_color = emission / optical_depth;
                    radiance += transmittance * (1.0 - segment_transmittance) * average_color;
                    transmittance *= segment_transmittance;
                }

                t += segment;
                steps += 1u;
                if (transmittance < MINIMUM_TRANSMITTANCE) {
                    complete = true;
                    break;
                }
            }
            if (!complete) {
                t = max(t, boundary);
            }
        }
    }

    return vec4<f32>(radiance, transmittance);
}
