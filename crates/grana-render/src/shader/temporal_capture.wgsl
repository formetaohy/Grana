struct CaptureData {
    region: vec4<u32>,
    local: vec4<u32>,
}

@group(0) @binding(0) var<uniform> capture: CaptureData;
@group(0) @binding(1) var accumulated: texture_2d<f32>;
@group(0) @binding(2) var counts: texture_2d<f32>;
@group(0) @binding(3) var object_depth: texture_depth_2d;
@group(0) @binding(4) var volume_normal: texture_2d<f32>;

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

fn surface_geometry(surface: vec2<i32>, local: vec2<i32>) -> vec4<f32> {
    let object = textureLoad(object_depth, surface, 0);
    let volume = textureLoad(volume_normal, surface, 0);
    if (object < 1.0 && (volume.w >= 1.0 || object <= volume.w)) {
        return vec4<f32>(0.0, 0.0, 0.0, object);
    }
    if (volume.w < 1.0) {
        return volume;
    }
    return vec4<f32>(0.0, 0.0, 0.0, 1.0);
}

@fragment
fn fs_main(input: VertexOutput) -> Captured {
    let surface = vec2<i32>(input.clip.xy);
    let local = surface - vec2<i32>(capture.region.xy);
    let count = max(textureLoad(counts, surface, 0).x, 1.0);
    let mean = textureLoad(accumulated, surface, 0) / count;
    return Captured(mean, surface_geometry(surface, local));
}
