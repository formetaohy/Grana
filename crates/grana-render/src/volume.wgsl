enable wgpu_binding_array;

const MAX_CHANNEL: u32 = 8u;
const BRICK_SIDE: f32 = 32.0;
const PADDED_SIDE: f32 = 34.0;

struct BrickRecord {
    slot: u32,
    reserved0: u32,
    reserved1: u32,
    reserved2: u32,
    maxima: array<vec4<f32>, 2>,
}

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

fn srgb_encode(linear: vec3<f32>) -> vec3<f32> {
    let clamped = clamp(linear, vec3<f32>(0.0), vec3<f32>(1.0));
    let low = clamped * 12.92;
    let high = 1.055 * pow(clamped, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(high, low, clamped <= vec3<f32>(0.0031308));
}

fn background(ndc: vec2<f32>) -> vec3<f32> {
    let height = clamp(ndc.y * 0.5 + 0.5, 0.0, 1.0);
    return mix(vec3<f32>(0.004, 0.005, 0.008), vec3<f32>(0.030, 0.038, 0.055), height);
}

fn channel_opacity(index: u32) -> f32 {
    return uniforms.channel_opacity[index / 4u][index % 4u];
}

fn channel_window(index: u32) -> f32 {
    return uniforms.channel_window[index / 4u][index % 4u];
}

fn channel_inverse(index: u32) -> f32 {
    return uniforms.channel_inverse[index / 4u][index % 4u];
}

fn lut_row(index: u32) -> f32 {
    return (f32(index) + 0.5) / f32(MAX_CHANNEL);
}

fn windowed(index: u32, unit: f32) -> f32 {
    return clamp((unit - channel_window(index)) * channel_inverse(index), 0.0, 1.0);
}

fn lut_sample(index: u32, unit: f32) -> vec4<f32> {
    return textureSampleLevel(lut, lut_sampler, vec2<f32>(windowed(index, unit), lut_row(index)), 0.0);
}

fn brick_cell(uvw: vec3<f32>) -> vec3<u32> {
    let grid = uniforms.grid.xyz;
    let scaled = clamp(uvw, vec3<f32>(0.0), vec3<f32>(1.0))
        * vec3<f32>(f32(grid.x), f32(grid.y), f32(grid.z));
    let rounded = floor(scaled);
    let cell = vec3<u32>(u32(rounded.x), u32(rounded.y), u32(rounded.z));
    return min(cell, grid - vec3<u32>(1u));
}

fn brick_index(cell: vec3<u32>) -> u32 {
    let grid = uniforms.grid.xyz;
    return cell.x + grid.x * (cell.y + grid.y * cell.z);
}

fn slot_origin(slot: u32) -> vec3<f32> {
    let grid = uniforms.atlas.xyz;
    let layer = slot / (grid.x * grid.y);
    let remainder = slot % (grid.x * grid.y);
    let row = remainder / grid.x;
    let column = remainder % grid.x;
    return vec3<f32>(f32(column), f32(row), f32(layer)) * PADDED_SIDE;
}

fn sample_channel(index: u32, uvw: vec3<f32>) -> f32 {
    let clamped = clamp(uvw, vec3<f32>(0.0), vec3<f32>(1.0));
    let cell = brick_cell(clamped);
    let record = bricks[brick_index(cell)];
    if (record.slot == 0u) {
        return 0.0;
    }
    let position = clamped
        * vec3<f32>(f32(uniforms.dims.x), f32(uniforms.dims.y), f32(uniforms.dims.z));
    let cell_position = vec3<f32>(f32(cell.x), f32(cell.y), f32(cell.z)) * BRICK_SIDE;
    let local = position - cell_position;
    let atlas_size = vec3<f32>(f32(uniforms.atlas.x), f32(uniforms.atlas.y), f32(uniforms.atlas.z)) * PADDED_SIDE;
    let texel = (slot_origin(record.slot - 1u) + local + vec3<f32>(1.5)) / atlas_size;
    return textureSampleLevel(atlases[index], atlas_sampler, texel, 0.0).r;
}

fn brick_transparent(record: BrickRecord) -> bool {
    let count = u32(uniforms.march.z);
    for (var channel = 0u; channel < count; channel += 1u) {
        if (channel_opacity(channel) > 0.0) {
            let maximum = record.maxima[channel / 4u][channel % 4u];
            let sample = lut_sample(channel, maximum);
            if (sample.a > 0.004) {
                return false;
            }
        }
    }
    return true;
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

fn shade(channel: u32, color: vec3<f32>, uvw: vec3<f32>, step: vec3<f32>, direction: vec3<f32>) -> vec3<f32> {
    let gradient = density_gradient(channel, uvw, step);
    if (length(gradient) < 1.0e-5) {
        return color;
    }
    var normal = -gradient / uniforms.box_size.xyz;
    normal = normal / length(normal);
    if (dot(normal, direction) > 0.0) {
        normal = -normal;
    }
    let to_eye = -direction;
    let half_vector = normalize(uniforms.light.xyz + to_eye);
    let diffuse = max(dot(normal, uniforms.light.xyz), 0.0);
    let specular = pow(max(dot(normal, half_vector), 0.0), uniforms.shading.w);
    return color * (uniforms.shading.x + uniforms.shading.y * diffuse) + vec3<f32>(specular * uniforms.shading.z);
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
    let exit = min(min(highest_t.x, highest_t.y), highest_t.z);
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

        var t = start;
        var steps = 0u;
        var cells = 0u;
        while (t < exit && steps < max_steps && cells < 4096u) {
            cells += 1u;
            let uvw = (eye + direction * t - box_min) / box_size;
            let cell = brick_cell(uvw);
            let record = bricks[brick_index(cell)];
            let boundary = min(t + boundary_delta(uvw, direction_uvw, step_length * 0.25), exit);
            if (record.slot == 0u || brick_transparent(record)) {
                t = boundary;
                continue;
            }
            let voxels_per_step = step_length * length(direction / voxel_world);
            while (t < boundary && steps < max_steps) {
                let position = (eye + direction * t - box_min) / box_size;
                var optical_depth = 0.0;
                var emission = vec3<f32>(0.0);
                for (var channel = 0u; channel < channel_count; channel += 1u) {
                    let opacity = channel_opacity(channel);
                    if (opacity > 0.0) {
                        let unit = sample_channel(channel, position);
                        let sample = lut_sample(channel, unit);
                        let per_voxel = clamp(sample.a, 0.0, 0.999);
                        if (per_voxel > 1.0e-4) {
                            let tau = -log(1.0 - per_voxel) * voxels_per_step * opacity;
                            var color = sample.rgb;
                            if (per_voxel > 0.02) {
                                color = shade(channel, color, position, gradient_step, direction);
                            }
                            optical_depth += tau;
                            emission += tau * color;
                        }
                    }
                }
                if (optical_depth > 0.0) {
                    let step_transmittance = exp(-optical_depth);
                    radiance += transmittance * (1.0 - step_transmittance) * emission / optical_depth;
                    transmittance *= step_transmittance;
                }
                if (transmittance < 0.003) {
                    break;
                }
                t += step_length;
                steps += 1u;
            }
            if (transmittance < 0.003) {
                break;
            }
            t = max(t, boundary);
        }
    }

    return vec4<f32>(srgb_encode(radiance + transmittance * background(input.ndc)), 1.0);
}
