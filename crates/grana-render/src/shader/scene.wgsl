struct FrameData {
    view_projection: mat4x4<f32>,
    inv_view_projection: mat4x4<f32>,
    eye: vec4<f32>,
    light: vec4<f32>,
    right: vec4<f32>,
    up: vec4<f32>,
    forward: vec4<f32>,
    box_min: vec4<f32>,
    box_size: vec4<f32>,
    dims: vec4<f32>,
    grid: vec4<u32>,
    atlas: vec4<u32>,
    shading: vec4<f32>,
    march: vec4<f32>,
    viewport: vec4<f32>,
    planes: array<vec4<f32>, 6>,
}

struct SectionData {
    uv_center: vec4<f32>,
    uv_scale: vec4<f32>,
    plane: vec4<f32>,
    mapping: vec4<u32>,
    box_min: vec4<f32>,
    world: vec4<f32>,
    dims: vec4<f32>,
    grid: vec4<u32>,
    atlas: vec4<u32>,
    axes: vec4<u32>,
}

struct ChannelData {
    opacity: f32,
    start: f32,
    inverse: f32,
    lut_row: f32,
}

@group(0) @binding(0) var<uniform> frame: FrameData;
@group(0) @binding(1) var<uniform> section: SectionData;
@group(0) @binding(2) var<storage, read> channels: array<ChannelData>;
@group(0) @binding(3) var lut: texture_2d<f32>;
@group(0) @binding(4) var lut_sampler: sampler;

fn channel_opacity(index: u32) -> f32 {
    return channels[index].opacity;
}

fn channel_sample(index: u32, unit: f32) -> vec4<f32> {
    let channel = channels[index];
    let position = clamp((unit - channel.start) * channel.inverse, 0.0, 1.0);
    return textureSampleLevel(
        lut,
        lut_sampler,
        vec2<f32>(position, channel.lut_row),
        0.0,
    );
}
