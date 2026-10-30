struct Uniforms {
    inv_view_projection: mat4x4<f32>,
    eye: vec4<f32>,
    box_min: vec4<f32>,
    box_size: vec4<f32>,
    shading: vec4<f32>,
    screen: vec4<f32>,
    march: vec4<f32>,
    light: vec4<f32>,
}

@group(0) @binding(0) var<uniform> uniforms: Uniforms;
@group(0) @binding(1) var volume_texture: texture_3d<f32>;
@group(0) @binding(2) var volume_sampler: sampler;
@group(0) @binding(3) var lut_texture: texture_2d<f32>;
@group(0) @binding(4) var lut_sampler: sampler;

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

fn gradient(uvw: vec3<f32>, texel: vec3<f32>) -> vec3<f32> {
    let step_x = vec3<f32>(texel.x, 0.0, 0.0);
    let step_y = vec3<f32>(0.0, texel.y, 0.0);
    let step_z = vec3<f32>(0.0, 0.0, texel.z);
    let x = textureSampleLevel(volume_texture, volume_sampler, uvw + step_x, 0.0).r
        - textureSampleLevel(volume_texture, volume_sampler, uvw - step_x, 0.0).r;
    let y = textureSampleLevel(volume_texture, volume_sampler, uvw + step_y, 0.0).r
        - textureSampleLevel(volume_texture, volume_sampler, uvw - step_y, 0.0).r;
    let z = textureSampleLevel(volume_texture, volume_sampler, uvw + step_z, 0.0).r
        - textureSampleLevel(volume_texture, volume_sampler, uvw - step_z, 0.0).r;
    return vec3<f32>(x, y, z);
}

fn shade(color: vec3<f32>, uvw: vec3<f32>, texel: vec3<f32>, direction: vec3<f32>) -> vec3<f32> {
    let density_gradient = gradient(uvw, texel);
    let magnitude = length(density_gradient);
    if (magnitude < 1e-5) {
        return color;
    }
    var normal = -density_gradient / uniforms.box_size.xyz;
    normal = normal / length(normal);
    if (dot(normal, direction) > 0.0) {
        normal = -normal;
    }
    let to_eye = -direction;
    let half_vector = normalize(uniforms.light.xyz + to_eye);
    let diffuse = max(dot(normal, uniforms.light.xyz), 0.0);
    let specular = pow(max(dot(normal, half_vector), 0.0), uniforms.shading.w);
    return color * (uniforms.shading.x + uniforms.shading.y * diffuse) + specular * uniforms.shading.z;
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
        let voxel = uniforms.box_size.xyz / vec3<f32>(textureDimensions(volume_texture));
        let min_voxel = uniforms.screen.w;
        let pixel_world = 2.0 * max(start, 1e-4) * uniforms.screen.z / max(uniforms.screen.y, 1.0);
        let step_length = max(pixel_world * 0.5, min_voxel * uniforms.march.x);
        let lod = clamp(log2(step_length / min_voxel), 0.0, uniforms.march.y);
        let voxels_per_step = step_length * length(direction / voxel);
        let texel = vec3<f32>(1.0) / vec3<f32>(textureDimensions(volume_texture));
        let jitter = interleaved_gradient_noise(input.clip_position.xy) * step_length;
        let steps = min(u32(ceil((exit - start) / step_length)), u32(uniforms.march.w));

        var t = start + jitter;
        for (var index = 0u; index < steps; index += 1u) {
            if (transmittance < 0.003) {
                break;
            }
            let uvw = (eye + direction * t - box_min) / uniforms.box_size.xyz;
            let value = textureSampleLevel(volume_texture, volume_sampler, uvw, lod).r;
            let sample = textureSampleLevel(lut_texture, lut_sampler, vec2<f32>(value, 0.5), 0.0);
            let per_voxel = clamp(sample.a * uniforms.march.z, 0.0, 0.999);
            let alpha = 1.0 - exp(log(1.0 - per_voxel) * voxels_per_step);
            if (alpha > 1e-4) {
                var color = sample.rgb;
                if (per_voxel > 0.02) {
                    color = shade(color, uvw, texel, direction);
                }
                radiance += transmittance * alpha * color;
                transmittance *= 1.0 - alpha;
            }
            t += step_length;
        }
    }

    return vec4<f32>(srgb_encode(radiance + transmittance * background(input.ndc)), 1.0);
}
