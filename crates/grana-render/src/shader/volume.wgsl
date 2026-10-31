@group(3) @binding(0) var object_depth: texture_depth_2d;
@group(4) @binding(0) var light_volume: texture_3d<f32>;
@group(4) @binding(1) var light_sampler: sampler;

const MAX_CELL_STEPS: u32 = 4096u;
const MINIMUM_TRANSMITTANCE: f32 = 0.003;
const MINIMUM_VOXEL_ALPHA: f32 = 1.0e-4;
const AMBIENT_OCCLUSION: f32 = 0.55;
const SHADING_THRESHOLD: f32 = 0.02;
const SHADING_FLOOR: f32 = 0.02;
const SHADING_CEIL: f32 = 0.10;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) ndc: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    let corners = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    var output: VertexOutput;
    output.clip_position = vec4<f32>(corners[index], 0.0, 1.0);
    output.ndc = corners[index];
    return output;
}

fn object_distance(depth: f32, ndc: vec2<f32>) -> f32 {
    if (depth >= 1.0) {
        return 1.0e20;
    }
    return length(unproject(vec3<f32>(ndc, depth)) - frame.eye.xyz);
}

fn region_interval(origin: vec3<f32>, direction: vec3<f32>) -> vec2<f32> {
    if (!region_active()) {
        return vec2<f32>(-1.0e20, 1.0e20);
    }
    var entry = -1.0e20;
    var exit = 1.0e20;
    for (var axis = 0u; axis < 3u; axis += 1u) {
        let step = direction[axis];
        let low = frame.region_min[axis];
        let high = frame.region_max[axis];
        if (abs(step) < 1.0e-9) {
            if (origin[axis] < low || origin[axis] > high) {
                return vec2<f32>(1.0, -1.0);
            }
        } else {
            let inverse = 1.0 / step;
            let first = (low - origin[axis]) * inverse;
            let second = (high - origin[axis]) * inverse;
            entry = max(entry, min(first, second));
            exit = min(exit, max(first, second));
        }
    }
    return vec2<f32>(entry, exit);
}

fn lighting(uvw: vec3<f32>) -> vec2<f32> {
    let strength = clamp(frame.march.y, 0.0, 1.0);
    if (strength <= 0.0) {
        return vec2<f32>(1.0, 0.0);
    }
    let sampled = textureSampleLevel(
        light_volume,
        light_sampler,
        clamp(uvw, vec3<f32>(0.0), vec3<f32>(1.0)),
        0.0,
    );
    return vec2<f32>(mix(1.0, sampled.r, strength), sampled.g * strength);
}

fn lit(color: vec3<f32>, normal: vec3<f32>, direction: vec3<f32>, light: vec2<f32>) -> vec3<f32> {
    if (length(normal) < 0.5) {
        return color;
    }
    let half_vector = normalize(frame.light.xyz - direction);
    let diffuse = max(dot(normal, frame.light.xyz), 0.0) * light.x;
    let specular = pow(max(dot(normal, half_vector), 0.0), frame.shading.w) * light.x;
    let ambient = frame.shading.x * (1.0 - AMBIENT_OCCLUSION * light.y);
    return color * (ambient + frame.shading.y * diffuse) + vec3<f32>(frame.shading.z * specular);
}

fn shade_volume(
    channel: u32,
    color: vec3<f32>,
    uvw: vec3<f32>,
    step: vec3<f32>,
    direction: vec3<f32>,
) -> vec3<f32> {
    let gradient = density_gradient(channel, uvw, step);
    let magnitude = length(gradient);
    if (magnitude < GRADIENT_EPSILON) {
        return color;
    }
    let weight = clamp(
        (magnitude - SHADING_FLOOR) / (SHADING_CEIL - SHADING_FLOOR),
        0.0,
        1.0,
    );
    if (weight <= 0.0) {
        return color;
    }
    return mix(
        color,
        lit(color, shaded_normal(gradient, direction), direction, lighting(uvw)),
        weight,
    );
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let eye = frame.eye.xyz;
    let direction = normalize(unproject(vec3<f32>(input.ndc, 1.0)) - eye);
    let box_min = frame.box_min.xyz;
    let box_size = frame.box_size.xyz;
    let box_max = box_min + box_size;

    let inverse_direction = vec3<f32>(1.0) / direction;
    let near_t = (box_min - eye) * inverse_direction;
    let far_t = (box_max - eye) * inverse_direction;
    let lowest_t = min(near_t, far_t);
    let highest_t = max(near_t, far_t);
    let entry = max(max(lowest_t.x, lowest_t.y), lowest_t.z);
    let object = object_distance(
        textureLoad(object_depth, vec2<i32>(input.clip_position.xy), 0),
        input.ndc,
    );
    let clip = region_interval(eye, direction);
    let exit = min(
        min(min(highest_t.x, highest_t.y), highest_t.z),
        min(object, clip.y),
    );
    let start = max(max(entry, 0.0), clip.x);

    var radiance = vec3<f32>(0.0);
    var transmittance = 1.0;

    if (exit > start) {
        let step_length = frame.march.x;
        let max_steps = 4096u;
        let channel_count = u32(frame.march.z);
        let voxel_world = box_size / frame.dims.xyz;
        let gradient_step = vec3<f32>(1.0) / frame.dims.xyz;
        let direction_uvw = direction / box_size;
        let voxels_per_world = length(direction / voxel_world);
        let jitter = interleaved_gradient(input.clip_position.xy) * step_length;

        var t = start + min(jitter, exit - start);
        var steps = 0u;
        var cells = 0u;
        var complete = false;
        while (t < exit && steps < max_steps && cells < MAX_CELL_STEPS && !complete) {
            cells += 1u;
            let uvw = ray_uvw(t, direction);
            let cell = brick_cell(frame.grid.xyz, uvw);
            let record = bricks[brick_index(frame.grid.xyz, cell)];
            let boundary = min(t + boundary_delta(uvw, direction_uvw, step_length * 0.25), exit);
            if (record.slot == 0u || record.flags == 0u) {
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
                    let unit = sample_volume(channel, position);
                    let sample = channel_sample(channel, unit);
                    let per_voxel = clamp(sample.a, 0.0, 0.999);
                    if (per_voxel > MINIMUM_VOXEL_ALPHA) {
                        let tau = -log(1.0 - per_voxel) * segment * voxels_per_world * opacity;
                        var color = sample.rgb;
                        if (per_voxel > SHADING_THRESHOLD) {
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
