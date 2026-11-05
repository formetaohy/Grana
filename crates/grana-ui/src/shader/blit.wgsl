struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var source_sampler: sampler;

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

@fragment
fn fs_blit(in: VertexOut) -> @location(0) vec4<f32> {
    let color = textureSampleLevel(source, source_sampler, in.uv, 0.0).rgb;
    return vec4<f32>(color, 1.0);
}

fn linear_to_srgb(channel: f32) -> f32 {
    if (channel <= 0.0031308) {
        return channel * 12.92;
    }
    return 1.055 * pow(channel, 1.0 / 2.4) - 0.055;
}

@fragment
fn fs_present_encoded(in: VertexOut) -> @location(0) vec4<f32> {
    let color = textureSampleLevel(source, source_sampler, in.uv, 0.0).rgb;
    return vec4<f32>(
        linear_to_srgb(color.r),
        linear_to_srgb(color.g),
        linear_to_srgb(color.b),
        1.0,
    );
}

@fragment
fn fs_present_linear(in: VertexOut) -> @location(0) vec4<f32> {
    let color = textureSampleLevel(source, source_sampler, in.uv, 0.0).rgb;
    return vec4<f32>(color, 1.0);
}
