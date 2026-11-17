struct ScreenUniforms {
    flags: vec4<f32>,
    fade: vec4<f32>,
    source: vec4<f32>,
}

@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var<uniform> screen: ScreenUniforms;
@group(0) @binding(2) var previous: texture_2d<f32>;
@group(0) @binding(3) var screen_sampler: sampler;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    let corners = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    var output: VertexOutput;
    output.clip_position = vec4<f32>(corners[index], 0.0, 1.0);
    output.uv = vec2<f32>(corners[index].x * 0.5 + 0.5, 0.5 - corners[index].y * 0.5);
    return output;
}

@fragment
fn fs_present(input: VertexOutput) -> @location(0) vec4<f32> {
    let pixel = input.clip_position.xy;
    let uv = screen.source.xy + input.uv * screen.source.zw;
    var resolved = textureSampleLevel(source, screen_sampler, uv, 0.0);
    let weight = screen.fade.x;
    if (weight > 0.0) {
        resolved = mix(textureSampleLevel(previous, screen_sampler, uv, 0.0), resolved, weight);
    }
    let coverage = clamp(resolved.a, 0.0, 1.0);
    let straight = resolved.rgb / max(resolved.a, 1.0e-4);
    if (screen.flags.z > 0.5) {
        let encoded = clamp(srgb_encode(aces(straight * screen.flags.y)), vec3<f32>(0.0), vec3<f32>(1.0));
        let grained = select((dither(pixel) - 0.5) / 255.0, 0.0, coverage <= 0.0 || screen.flags.w < 0.5);
        let value = clamp(encoded + vec3<f32>(grained), vec3<f32>(0.0), vec3<f32>(1.0));
        return vec4<f32>(select(srgb_decode(value), value, screen.flags.x > 0.5), coverage);
    }
    let mapped = aces(straight * screen.flags.y) * coverage;
    let encoded = clamp(srgb_encode(mapped), vec3<f32>(0.0), vec3<f32>(1.0));
    let grained = select((dither(pixel) - 0.5) / 255.0, 0.0, coverage <= 0.0 || screen.flags.w < 0.5);
    let value = clamp(encoded + vec3<f32>(grained), vec3<f32>(0.0), vec3<f32>(1.0));
    return vec4<f32>(select(srgb_decode(value), value, screen.flags.x > 0.5), 1.0);
}
