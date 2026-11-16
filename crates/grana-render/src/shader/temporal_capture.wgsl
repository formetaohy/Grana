@group(2) @binding(0) var accumulated: texture_2d<f32>;
@group(2) @binding(1) var counts: texture_2d<f32>;

struct VertexOutput {
    @builtin(position) clip: vec4<f32>,
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
    output.clip = vec4<f32>(corners[index], 0.0, 1.0);
    output.ndc = corners[index];
    return output;
}

struct Captured {
    @location(0) color: vec4<f32>,
    @location(1) geometry: vec4<f32>,
}

@fragment
fn fs_main(input: VertexOutput) -> Captured {
    let pixel = vec2<i32>(input.clip.xy);
    let count = max(textureLoad(counts, pixel, 0).x, 1.0);
    let mean = textureLoad(accumulated, pixel, 0) / count;
    return Captured(mean, geometry_of(camera_surface(pixel, input.ndc)));
}
