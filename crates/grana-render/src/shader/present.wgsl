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
fn fs_background(input: VertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(background(input.ndc), 1.0);
}

@fragment
fn fs_present(input: VertexOutput) -> @location(0) vec4<f32> {
    let axis = max(i32(screen.flags.z + 0.5), 1);
    let origin = vec2<i32>(input.clip_position.xy) * axis;
    var radiance = vec3<f32>(0.0);
    for (var y = 0; y < axis; y += 1) {
        for (var x = 0; x < axis; x += 1) {
            radiance += textureLoad(scene_color, origin + vec2<i32>(x, y), 0).rgb;
        }
    }
    let resolved = radiance / f32(axis * axis);
    let mapped = aces(resolved * screen.flags.y);
    let noise = (dither(input.clip_position.xy) - 0.5) / 255.0;
    let encoded = clamp(srgb_encode(mapped) + vec3<f32>(noise), vec3<f32>(0.0), vec3<f32>(1.0));
    return vec4<f32>(select(srgb_decode(encoded), encoded, screen.flags.x > 0.5), 1.0);
}
