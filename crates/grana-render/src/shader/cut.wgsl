@group(0) @binding(0) var layer: texture_2d<f32>;
@group(0) @binding(1) var layer_sampler: sampler;

struct Output {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> Output {
    let corners = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    let corner = corners[index];
    var output: Output;
    output.clip_position = vec4<f32>(corner, 0.0, 1.0);
    output.uv = vec2<f32>(corner.x * 0.5 + 0.5, 0.5 - corner.y * 0.5);
    return output;
}

@fragment
fn fs_main(input: Output) -> @location(0) vec4<f32> {
    return textureSampleLevel(layer, layer_sampler, input.uv, 0.0);
}
