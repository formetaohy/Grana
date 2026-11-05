@group(0) @binding(0) var scene_depth: texture_depth_multisampled_2d;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
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
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @builtin(frag_depth) f32 {
    let pixel = vec2<i32>(input.clip_position.xy);
    var nearest = 1.0;
    let samples = textureNumSamples(scene_depth);
    for (var sample = 0u; sample < samples; sample += 1u) {
        nearest = min(nearest, textureLoad(scene_depth, pixel, sample));
    }
    return nearest;
}
