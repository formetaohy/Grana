struct BlurUniforms {
    base: vec4<f32>,
    limits: vec4<f32>,
    step: vec2<f32>,
    radius: f32,
    saturation: f32,
};

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var source_sampler: sampler;
@group(0) @binding(2) var<uniform> uniforms: BlurUniforms;

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

fn saturate_color(color: vec3<f32>, amount: f32) -> vec3<f32> {
    let luminance = vec3<f32>(0.213, 0.715, 0.072);
    return clamp(mix(vec3<f32>(dot(color, luminance)), color, amount), vec3<f32>(0.0), vec3<f32>(1.0));
}

@fragment
fn fs_box(in: VertexOut) -> @location(0) vec4<f32> {
    let radius = uniforms.radius;
    let origin = uniforms.base.xy + in.uv * uniforms.base.zw;
    var sum = vec3<f32>(0.0);
    var offset = -radius;
    var taps = 0.0;
    loop {
        if (offset > radius + 0.001) {
            break;
        }
        let position = clamp(origin + uniforms.step * offset, uniforms.limits.xy, uniforms.limits.zw);
        sum += textureSampleLevel(source, source_sampler, position, 0.0).rgb;
        offset += 1.0;
        taps += 1.0;
    }
    let color = sum / max(taps, 1.0);
    return vec4<f32>(saturate_color(color, uniforms.saturation), 1.0);
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
