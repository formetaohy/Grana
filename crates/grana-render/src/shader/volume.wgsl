@group(3) @binding(0) var object_depth: texture_depth_2d;
@group(4) @binding(0) var shadow_tau: texture_3d<f32>;
@group(4) @binding(1) var shadow_sampler: sampler;
@group(4) @binding(2) var shadow_occlusion: texture_3d<f32>;
@group(4) @binding(3) var shadow_objects: texture_depth_2d;
@group(4) @binding(4) var shadow_compare: sampler_comparison;

@group(5) @binding(0) var<storage, read> oit_keys: array<u32>;
@group(5) @binding(1) var<storage, read> oit_colors: array<vec2<u32>>;
@group(5) @binding(2) var<storage, read> oit_counts: array<u32>;
@group(5) @binding(3) var oit_accumulate: texture_2d<f32>;
@group(5) @binding(4) var oit_reveal: texture_2d<f32>;
@group(5) @binding(5) var oit_tail_depth: texture_2d<f32>;

const MAX_CELL_STEPS: u32 = 4096u;
const MINIMUM_TRANSMITTANCE: f32 = 0.003;
const MINIMUM_VOXEL_ALPHA: f32 = 1.0e-4;
const AMBIENT_OCCLUSION: f32 = 0.55;
const SHADING_THRESHOLD: f32 = 0.02;
const SHADING_FLOOR: f32 = 0.02;
const SHADING_CEIL: f32 = 0.10;
const EMPTY_DISTANCE: f32 = 1.0e19;
const MAXIMUM_LAYERS: u32 = 8u;
const TAIL_DEPTH_SCALE: f32 = 64.0;

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
        return EMPTY_DISTANCE;
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

fn lighting(uvw: vec3<f32>, normal: vec3<f32>) -> vec2<f32> {
    let occlusion = local_occlusion(uvw);
    let strength = clamp(frame.march.y, 0.0, 1.0);
    if (strength <= 0.0) {
        return vec2<f32>(1.0, occlusion);
    }
    let world = frame.box_min.xyz + uvw * frame.box_size.xyz;
    let coordinate = shadow_coordinate(frame.light_view, world);
    let sun = sun_transmittance(
        coordinate,
        shadow_bias(frame.light_reach, normal, frame.light.xyz),
    );
    return vec2<f32>(mix(1.0, sun, strength), occlusion);
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
    let normal = shaded_normal(gradient, direction);
    return mix(color, lit(color, normal, direction, lighting(uvw, normal)), weight);
}

fn march(begin: f32, finish: f32, direction: vec3<f32>) -> vec4<f32> {
    var radiance = vec3<f32>(0.0);
    var transmittance = 1.0;
    if (finish > begin) {
        let step_length = frame.march.x;
        let max_steps = 4096u;
        let channel_count = u32(frame.march.z);
        let voxel_world = frame.box_size.xyz / frame.dims.xyz;
        let gradient_step = vec3<f32>(1.0) / frame.dims.xyz;
        let direction_uvw = direction / frame.box_size.xyz;
        let voxels_per_world = length(direction / voxel_world);

        var t = begin;
        var steps = 0u;
        var cells = 0u;
        var complete = false;
        while (t < finish && steps < max_steps && cells < MAX_CELL_STEPS && !complete) {
            cells += 1u;
            let uvw = ray_uvw(t, direction);
            let cell = brick_cell(frame.grid.xyz, frame.dims.xyz, uvw);
            let record = bricks[brick_index(frame.grid.xyz, cell)];
            let boundary = min(t + boundary_delta(uvw, direction_uvw, step_length * 0.25), finish);
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

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let eye = frame.eye.xyz;
    let ndc = input.ndc + frame.jitter.xy;
    let pixel = vec2<i32>(input.clip_position.xy);
    let direction = normalize(unproject(vec3<f32>(ndc, 1.0)) - eye);
    let box_min = frame.box_min.xyz;
    let box_max = box_min + frame.box_size.xyz;

    let inverse_direction = vec3<f32>(1.0) / direction;
    let near_t = (box_min - eye) * inverse_direction;
    let far_t = (box_max - eye) * inverse_direction;
    let lowest_t = min(near_t, far_t);
    let highest_t = max(near_t, far_t);
    let entry = max(max(lowest_t.x, lowest_t.y), lowest_t.z);
    let object = object_distance(textureLoad(object_depth, pixel, 0), ndc);
    let clip = region_interval(eye, direction);
    let exit = min(
        min(min(highest_t.x, highest_t.y), highest_t.z),
        min(min(object, clip.y), 1.0e19),
    );
    let start = max(max(entry, 0.0), clip.x);
    let jitter = sample_noise(input.clip_position.xy, frame.jitter.z) * frame.march.x;

    var layer_depth: array<f32, MAXIMUM_LAYERS>;
    var layer_color: array<vec2<u32>, MAXIMUM_LAYERS>;
    var layers = 0u;
    var tail = vec4<f32>(0.0);
    var tail_depth = EMPTY_DISTANCE;
    if (frame.oit.x > 0u) {
        let origin = vec2<i32>(frame.viewport.xy);
        let size = vec2<i32>(frame.viewport.zw);
        let local = pixel - origin;
        if (local.x >= 0 && local.y >= 0 && local.x < size.x && local.y < size.y) {
            let index = u32(local.y) * u32(size.x) + u32(local.x);
            let base = index * frame.oit.y;
            let stored = min(oit_counts[index], frame.oit.y);
            for (var slot = 0u; slot < stored; slot += 1u) {
                let key = oit_keys[base + slot];
                let depth = bitcast<f32>(~key);
                var position = layers;
                loop {
                    if (position == 0u || layer_depth[position - 1u] <= depth) {
                        break;
                    }
                    layer_depth[position] = layer_depth[position - 1u];
                    layer_color[position] = layer_color[position - 1u];
                    position -= 1u;
                }
                layer_depth[position] = depth;
                layer_color[position] = oit_colors[base + slot];
                layers += 1u;
            }
            let accumulation = textureLoad(oit_accumulate, pixel, 0);
            if (accumulation.a > 0.0) {
                let reveal = textureLoad(oit_reveal, pixel, 0).r;
                let weighted = textureLoad(oit_tail_depth, pixel, 0).r;
                tail = vec4<f32>(accumulation.rgb / accumulation.a, 1.0 - reveal);
                tail_depth = weighted * TAIL_DEPTH_SCALE / accumulation.a;
            }
        }
    }

    var radiance = vec3<f32>(0.0);
    var transmittance = 1.0;
    var cursor = start + min(jitter, max(exit - start, 0.0));
    for (var index = 0u; index < layers; index += 1u) {
        let limit = min(object_distance(layer_depth[index], ndc), exit);
        if (limit > cursor) {
            let segment = march(cursor, limit, direction);
            radiance += transmittance * segment.rgb;
            transmittance *= segment.a;
            cursor = limit;
        }
        let sample = vec4<f32>(
            unpack2x16float(layer_color[index].x),
            unpack2x16float(layer_color[index].y),
        );
        radiance += transmittance * sample.rgb;
        transmittance *= 1.0 - sample.a;
        if (transmittance < MINIMUM_TRANSMITTANCE) {
            break;
        }
    }
    if (tail.a > 0.0) {
        let limit = min(object_distance(tail_depth, ndc), exit);
        if (limit > cursor) {
            let segment = march(cursor, limit, direction);
            radiance += transmittance * segment.rgb;
            transmittance *= segment.a;
            cursor = limit;
        }
        radiance += transmittance * tail.rgb * tail.a;
        transmittance *= 1.0 - tail.a;
    }
    if (cursor < exit) {
        let segment = march(cursor, exit, direction);
        radiance += transmittance * segment.rgb;
        transmittance *= segment.a;
    }
    return vec4<f32>(radiance, 1.0 - transmittance);
}
