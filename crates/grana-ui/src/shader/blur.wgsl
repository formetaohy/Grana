struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_fullscreen(@builtin(vertex_index) index: u32) -> VertexOut {
    var corners = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    let corner = corners[index];
    var out: VertexOut;
    out.position = vec4<f32>(corner, 0.0, 1.0);
    out.uv = vec2<f32>(corner.x * 0.5 + 0.5, 0.5 - corner.y * 0.5);
    return out;
}

@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var source_sampler: sampler;

@fragment
fn fs_downsample(in: VertexOut) -> @location(0) vec4<f32> {
    let texel = 1.0 / vec2<f32>(textureDimensions(source));
    let offset = texel * 0.5;
    var sum = textureSampleLevel(source, source_sampler, in.uv + vec2<f32>(-offset.x, -offset.y), 0.0);
    sum += textureSampleLevel(source, source_sampler, in.uv + vec2<f32>(offset.x, -offset.y), 0.0);
    sum += textureSampleLevel(source, source_sampler, in.uv + vec2<f32>(-offset.x, offset.y), 0.0);
    sum += textureSampleLevel(source, source_sampler, in.uv + vec2<f32>(offset.x, offset.y), 0.0);
    return sum * 0.25;
}

@fragment
fn fs_blit(in: VertexOut) -> @location(0) vec4<f32> {
    let color = textureSampleLevel(source, source_sampler, in.uv, 0.0);
    return vec4<f32>(color.rgb, 1.0);
}

fn srgb_to_linear(channel: f32) -> f32 {
    if (channel <= 0.04045) {
        return channel / 12.92;
    }
    return pow((channel + 0.055) / 1.055, 2.4);
}

@fragment
fn fs_blit_linear(in: VertexOut) -> @location(0) vec4<f32> {
    let color = textureSampleLevel(source, source_sampler, in.uv, 0.0);
    return vec4<f32>(
        srgb_to_linear(color.r),
        srgb_to_linear(color.g),
        srgb_to_linear(color.b),
        1.0,
    );
}
