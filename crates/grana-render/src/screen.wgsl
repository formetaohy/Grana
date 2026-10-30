struct ScreenUniforms {
    flags: vec4<f32>,
}

@group(0) @binding(0) var scene_color: texture_2d<f32>;
@group(0) @binding(1) var<uniform> screen: ScreenUniforms;

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

@fragment
fn fs_background(input: VertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(background(input.ndc), 1.0);
}

@fragment
fn fs_present(input: VertexOutput) -> @location(0) vec4<f32> {
    let linear = textureLoad(scene_color, vec2<i32>(input.clip_position.xy), 0).rgb;
    let noise = (dither(input.clip_position.xy) - 0.5) / 255.0;
    let encoded = clamp(srgb_encode(linear) + vec3<f32>(noise), vec3<f32>(0.0), vec3<f32>(1.0));
    return vec4<f32>(select(srgb_decode(encoded), encoded, screen.flags.x > 0.5), 1.0);
}
