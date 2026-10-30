enable wgpu_binding_array;

const MAX_CHANNEL: u32 = 8u;

struct Uniforms {
    inv_view_projection: mat4x4<f32>,
    eye: vec4<f32>,
    box_min: vec4<f32>,
    box_size: vec4<f32>,
    shading: vec4<f32>,
    screen: vec4<f32>,
    march: vec4<f32>,
    light: vec4<f32>,
    channel_opacity: array<vec4<f32>, 2>,
}

@group(0) @binding(0) var<uniform> uniforms: Uniforms;
@group(0) @binding(1) var lut: texture_2d<f32>;
@group(0) @binding(2) var lut_sampler: sampler;
@group(1) @binding(0) var volumes: binding_array<texture_3d<f32>, 8>;
@group(1) @binding(1) var volume_sampler: sampler;

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

fn interleaved_gradient_noise(position: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(position, vec2<f32>(0.06711056, 0.00583715))));
}

fn channel_opacity(index: u32) -> f32 {
    let word = uniforms.channel_opacity[index / 4u];
    return word[index % 4u];
}

fn lut_row(index: u32) -> f32 {
    return (f32(index) + 0.5) / f32(MAX_CHANNEL);
}

fn density_gradient(channel: u32, uvw: vec3<f32>, step: vec3<f32>, lod: f32) -> vec3<f32> {
    let x = textureSampleLevel(volumes[channel], volume_sampler, uvw + vec3<f32>(step.x, 0.0, 0.0), lod).r
        - textureSampleLevel(volumes[channel], volume_sampler, uvw - vec3<f32>(step.x, 0.0, 0.0), lod).r;
    let y = textureSampleLevel(volumes[channel], volume_sampler, uvw + vec3<f32>(0.0, step.y, 0.0), lod).r
        - textureSampleLevel(volumes[channel], volume_sampler, uvw - vec3<f32>(0.0, step.y, 0.0), lod).r;
    let z = textureSampleLevel(volumes[channel], volume_sampler, uvw + vec3<f32>(0.0, 0.0, step.z), lod).r
        - textureSampleLevel(volumes[channel], volume_sampler, uvw - vec3<f32>(0.0, 0.0, step.z), lod).r;
    return vec3<f32>(x, y, z);
}

fn shade(channel: u32, color: vec3<f32>, uvw: vec3<f32>, step: vec3<f32>, lod: f32, direction: vec3<f32>) -> vec3<f32> {
    let density_gradient_value = density_gradient(channel, uvw, step, lod);
    let magnitude = length(density_gradient_value);
    if (magnitude < 1e-5) {
        return color;
    }
    var normal = -density_gradient_value / uniforms.box_size.xyz;
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
    let box_max = box_min + uniforms.box_size.xyz;

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
        let dimensions = vec3<f32>(textureDimensions(volumes[0]));
        let voxel = uniforms.box_size.xyz / dimensions;
        let min_voxel = uniforms.screen.w;
        let pixel_world = 2.0 * max(start, 1e-4) * uniforms.screen.z / max(uniforms.screen.y, 1.0);
        let step_length = max(pixel_world * 0.5, min_voxel * uniforms.march.x);
        let lod = clamp(log2(step_length / min_voxel), 0.0, uniforms.march.y);
        let voxels_per_step = step_length * length(direction / voxel);
        let texel = vec3<f32>(1.0) / dimensions;
        let gradient_step = texel * exp2(lod);
        let jitter = interleaved_gradient_noise(input.clip_position.xy) * step_length;
        let steps = min(u32(ceil((exit - start) / step_length)), u32(uniforms.march.w));
        let channel_count = u32(uniforms.march.z);

        var t = start + jitter;
        for (var index = 0u; index < steps; index += 1u) {
            if (transmittance < 0.003) {
                break;
            }
            let uvw = (eye + direction * t - box_min) / uniforms.box_size.xyz;
            var optical_depth = 0.0;
            var emission = vec3<f32>(0.0);
            for (var channel = 0u; channel < channel_count; channel += 1u) {
                let opacity = channel_opacity(channel);
                if (opacity > 0.0) {
                    let density = textureSampleLevel(volumes[channel], volume_sampler, uvw, lod).r;
                    let sample = textureSampleLevel(lut, lut_sampler, vec2<f32>(density, lut_row(channel)), 0.0);
                    let per_voxel = clamp(sample.a, 0.0, 0.999);
                    if (per_voxel > 1e-4) {
                        let tau = -log(1.0 - per_voxel) * voxels_per_step * opacity;
                        var color = sample.rgb;
                        if (per_voxel > 0.02) {
                            color = shade(channel, color, uvw, gradient_step, lod, direction);
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
            t += step_length;
        }
    }

    return vec4<f32>(srgb_encode(radiance + transmittance * background(input.ndc)), 1.0);
}
