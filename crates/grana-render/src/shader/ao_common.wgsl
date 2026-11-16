struct AoData {
    view_projection: mat4x4<f32>,
    inverse_view_projection: mat4x4<f32>,
    eye: vec4<f32>,
    extent: vec4<f32>,
    region: vec4<f32>,
    noise: vec4<f32>,
}

@group(0) @binding(0) var<uniform> ao: AoData;

const PI: f32 = 3.14159265359;
const FLOOR: f32 = 1.0e-5;

fn world_of(ndc: vec2<f32>, depth: f32) -> vec3<f32> {
    let clip = ao.inverse_view_projection * vec4<f32>(ndc, depth, 1.0);
    return clip.xyz / clip.w;
}

fn pixel_world(world: vec3<f32>) -> f32 {
    let span = select(1.0, length(world - ao.eye.xyz), ao.noise.z > 0.5);
    return span / max(ao.noise.y, FLOOR);
}

fn region_ndc(pixel: vec2<f32>, region: vec4<f32>) -> vec2<f32> {
    let local = pixel - region.xy + vec2<f32>(0.5);
    return vec2<f32>(
        local.x / region.z * 2.0 - 1.0,
        1.0 - local.y / region.w * 2.0,
    );
}

fn region_texel(pixel: vec2<i32>) -> vec2<i32> {
    let low = vec2<i32>(ao.region.xy);
    let high = low + vec2<i32>(ao.region.zw) - vec2<i32>(1);
    return clamp(pixel, low, high);
}

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let corners = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    return vec4<f32>(corners[index], 0.0, 1.0);
}
