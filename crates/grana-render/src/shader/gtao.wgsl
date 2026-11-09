struct AoData {
    view_projection: mat4x4<f32>,
    inverse_view_projection: mat4x4<f32>,
    eye: vec4<f32>,
    extent: vec4<f32>,
    noise: vec4<f32>,
}

@group(0) @binding(0) var<uniform> ao: AoData;
@group(0) @binding(1) var surface_depth: texture_depth_2d;
@group(0) @binding(2) var surface_normal: texture_2d<f32>;
@group(0) @binding(3) var ao_output: texture_storage_2d<rgba8unorm, write>;

const SLICES: u32 = 8u;
const STEPS: u32 = 5u;
const PI: f32 = 3.14159265359;
const FLOOR: f32 = 1.0e-5;

fn world_of(ndc: vec2<f32>, depth: f32) -> vec3<f32> {
    let clip = ao.inverse_view_projection * vec4<f32>(ndc, depth, 1.0);
    return clip.xyz / clip.w;
}

fn pixel_of(world: vec3<f32>) -> vec4<f32> {
    let clip = ao.view_projection * vec4<f32>(world, 1.0);
    if (clip.w <= FLOOR) {
        return vec4<f32>(0.0);
    }
    let ndc = clip.xy / clip.w;
    return vec4<f32>(
        (ndc.x * 0.5 + 0.5) * ao.extent.x,
        (0.5 - ndc.y * 0.5) * ao.extent.y,
        clip.z / clip.w,
        1.0,
    );
}

fn ndc_of(pixel: vec2<i32>) -> vec2<f32> {
    return vec2<f32>(
        (f32(pixel.x) + 0.5) / ao.extent.x * 2.0 - 1.0,
        1.0 - (f32(pixel.y) + 0.5) / ao.extent.y * 2.0,
    );
}

fn dither(seed: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(seed, vec2<f32>(0.06711056, 0.00583715))));
}

fn store(pixel: vec2<i32>, value: f32) {
    textureStore(ao_output, pixel, vec4<f32>(value, 0.0, 0.0, 1.0));
}

@compute @workgroup_size(8, 8)
fn gtao_main(@builtin(global_invocation_id) id: vec3<u32>) {
    let width = u32(ao.extent.x);
    let height = u32(ao.extent.y);
    if (id.x >= width || id.y >= height) {
        return;
    }
    let pixel = vec2<i32>(id.xy);
    let depth = textureLoad(surface_depth, pixel, 0);
    let raw_normal = textureLoad(surface_normal, pixel, 0).xyz;
    if (depth >= 1.0 || length(raw_normal) < 0.5) {
        store(pixel, 1.0);
        return;
    }
    let position = world_of(ndc_of(pixel), depth);
    let view = normalize(ao.eye.xyz - position);
    let normal = normalize(raw_normal);
    let facing = select(normal, -normal, dot(normal, view) < 0.0);
    let across = cross(facing, view);
    let across_length = length(across);
    let tangent = select(
        vec3<f32>(1.0, 0.0, 0.0),
        across / across_length,
        across_length > FLOOR,
    );
    let bitangent = cross(facing, tangent);
    let focal = max(ao.noise.y, 1.0);
    let radius = ao.extent.z * length(position - ao.eye.xyz) / focal;
    let rotation = dither(vec2<f32>(f32(pixel.x), f32(pixel.y))) * 6.2831853
        + ao.noise.x * 2.39996323;
    var occlusion = 0.0;
    for (var slice = 0u; slice < SLICES; slice += 1u) {
        let angle = rotation + PI * f32(slice) / f32(SLICES);
        let direction = tangent * cos(angle) + bitangent * sin(angle);
        for (var side = 0u; side < 2u; side += 1u) {
            let signum = select(1.0, -1.0, side == 1u);
            var horizon = 0.0;
            for (var step = 1u; step <= STEPS; step += 1u) {
                let reach = radius * f32(step) / f32(STEPS);
                let probe = position + direction * (signum * reach);
                let projected = pixel_of(probe);
                if (projected.w < 0.5) {
                    continue;
                }
                let sample_pixel = vec2<i32>(floor(projected.xy));
                if (sample_pixel.x < 0
                    || sample_pixel.y < 0
                    || sample_pixel.x >= i32(width)
                    || sample_pixel.y >= i32(height)) {
                    continue;
                }
                let sample_depth = textureLoad(surface_depth, sample_pixel, 0);
                if (sample_depth >= 1.0) {
                    continue;
                }
                let occluder = world_of(ndc_of(sample_pixel), sample_depth);
                let offset = occluder - position;
                let distance = length(offset);
                if (distance < FLOOR || distance > radius * 1.5) {
                    continue;
                }
                let cosine = dot(offset / distance, facing);
                if (cosine <= 0.0) {
                    continue;
                }
                let falloff = 1.0 - clamp(distance / radius, 0.0, 1.0);
                horizon = max(horizon, cosine * falloff);
            }
            occlusion += horizon;
        }
    }
    store(pixel, clamp(1.0 - ao.extent.w * occlusion / f32(SLICES * 2u), 0.0, 1.0));
}
