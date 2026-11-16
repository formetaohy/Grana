struct MeshUniforms {
    view_projection: mat4x4<f32>,
    inverse_view_projection: mat4x4<f32>,
    eye: vec4<f32>,
    light: vec4<f32>,
    shading: vec4<f32>,
    region_center: vec4<f32>,
    region_axis: array<vec4<f32>, 3>,
    jitter: vec4<f32>,
    light_view: mat4x4<f32>,
    light_reach: vec4<f32>,
    shadow: vec4<f32>,
    viewport: vec4<f32>,
    oit: vec4<u32>,
    clip: vec4<f32>,
    forward: vec4<f32>,
    projection: vec4<f32>,
    lens: vec4<f32>,
}

struct StyleData {
    color: vec4<f32>,
    flags: vec4<f32>,
    surface: vec4<f32>,
}

@group(0) @binding(0) var<uniform> mesh: MeshUniforms;
@group(2) @binding(0) var<storage, read> styles: array<StyleData>;
@group(3) @binding(0) var shadow_tau: texture_3d<f32>;
@group(3) @binding(1) var shadow_sampler: sampler;
@group(3) @binding(3) var shadow_objects: texture_depth_2d;
@group(3) @binding(4) var shadow_compare: sampler_comparison;
@group(7) @binding(0) var ao_map: texture_2d<f32>;
