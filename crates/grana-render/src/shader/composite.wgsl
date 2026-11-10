@group(1) @binding(0) var scene_depth: texture_depth_2d;
@group(2) @binding(0) var volume_layer: texture_2d<f32>;

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

fn pane_local(pixel: vec2<f32>) -> vec2<f32> {
    return (pixel - frame.viewport.xy) / volume_scale();
}

fn tap_weight(centre_ndc: vec2<f32>, centre: vec3<f32>, depth: f32) -> f32 {
    let reach = 8.0 / max(frame.sampling.x, 1.0e-6);
    let gap = length(unproject(vec3<f32>(centre_ndc, depth)) - centre);
    let ratio = gap / reach;
    let squared = ratio * ratio;
    return 1.0 / (1.0 + squared * squared);
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let scale = volume_scale();
    let local = pane_local(input.clip_position.xy);
    let coord = local - vec2<f32>(0.5);
    let base = vec2<i32>(floor(coord));
    let fraction = coord - floor(coord);
    let dims = vec2<i32>(textureDimensions(volume_layer));
    let low = vec2<i32>(0);
    let high = max(dims - vec2<i32>(1), low);
    let here = vec2<i32>(floor(input.clip_position.xy));
    let centre_depth = textureLoad(scene_depth, here, 0);
    let centre = unproject(vec3<f32>(input.ndc, centre_depth));
    let background = centre_depth >= 1.0;
    let step = 2.0 / frame.viewport.zw;
    let edge_low = vec2<i32>(frame.viewport.xy);
    let edge_high = vec2<i32>(frame.viewport.xy + frame.viewport.zw) - vec2<i32>(1);
    var total = 0.0;
    var gathered = vec4<f32>(0.0);
    for (var index = 0u; index < 4u; index += 1u) {
        let corner = vec2<f32>(f32(index & 1u), f32(index >> 1u));
        let bilinear = select(vec2<f32>(1.0) - fraction, fraction, corner > vec2<f32>(0.5));
        var accepted = bilinear.x * bilinear.y;
        let tap = clamp(base + vec2<i32>(i32(index & 1u), i32(index >> 1u)), low, high);
        let across = (vec2<f32>(tap) + vec2<f32>(0.5) - local) * scale;
        let tap_pixel = clamp(
            vec2<i32>(floor(frame.viewport.xy + (vec2<f32>(tap) + vec2<f32>(0.5)) * scale)),
            edge_low,
            edge_high,
        );
        let depth = textureLoad(scene_depth, tap_pixel, 0);
        if (background != (depth >= 1.0)) {
            accepted = 0.0;
        } else if (!background) {
            accepted = accepted * tap_weight(input.ndc + across * step, centre, depth);
        }
        total += accepted;
        gathered += textureLoad(volume_layer, tap, 0) * accepted;
    }
    if (total <= 0.0) {
        return vec4<f32>(0.0);
    }
    return gathered / total;
}
