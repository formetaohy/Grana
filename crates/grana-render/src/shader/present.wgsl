struct ScreenUniforms {
    flags: vec4<f32>,
}

@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var<uniform> screen: ScreenUniforms;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) ndc: vec2<f32>,
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
    output.ndc = corners[index];
    return output;
}

@fragment
fn fs_accumulate(input: VertexOutput) -> @location(0) vec4<f32> {
    return textureLoad(source, vec2<i32>(input.clip_position.xy), 0);
}

@fragment
fn fs_present(input: VertexOutput) -> @location(0) vec4<f32> {
    let accumulated = textureLoad(source, vec2<i32>(input.clip_position.xy), 0) * screen.flags.z;
    let coverage = clamp(accumulated.a, 0.0, 1.0);
    let noise = (dither(input.clip_position.xy) - 0.5) / 255.0;
    if screen.flags.w > 0.5 {
        let straight = srgb_encode(aces(accumulated.rgb * screen.flags.y / max(coverage, 1.0e-4)));
        let encoded = clamp(straight + vec3<f32>(noise), vec3<f32>(0.0), vec3<f32>(1.0));
        return vec4<f32>(select(srgb_decode(encoded), encoded, screen.flags.x > 0.5), coverage);
    }
    let mapped =
        aces(accumulated.rgb * screen.flags.y) + aces(background(input.ndc)) * (1.0 - coverage);
    let encoded = clamp(srgb_encode(mapped) + vec3<f32>(noise), vec3<f32>(0.0), vec3<f32>(1.0));
    return vec4<f32>(select(srgb_decode(encoded), encoded, screen.flags.x > 0.5), 1.0);
}
