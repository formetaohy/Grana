struct ComposeData {
    flags: vec4<f32>,
}

@group(0) @binding(0) var picture: texture_2d<f32>;
@group(0) @binding(1) var overlay: texture_2d<f32>;
@group(0) @binding(2) var<uniform> data: ComposeData;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
}

fn srgb_encode(linear: vec3<f32>) -> vec3<f32> {
    let clamped = clamp(linear, vec3<f32>(0.0), vec3<f32>(1.0));
    let low = clamped * 12.92;
    let high = 1.055 * pow(clamped, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(high, low, clamped <= vec3<f32>(0.0031308));
}

fn srgb_decode(encoded: vec3<f32>) -> vec3<f32> {
    let clamped = clamp(encoded, vec3<f32>(0.0), vec3<f32>(1.0));
    let low = clamped / 12.92;
    let high = pow((clamped + 0.055) / 1.055, vec3<f32>(2.4));
    return select(high, low, clamped <= vec3<f32>(0.04045));
}

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    let corners = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    var output: VertexOutput;
    output.position = vec4<f32>(corners[index], 0.0, 1.0);
    return output;
}

@fragment
fn fs_compose(input: VertexOutput) -> @location(0) vec4<f32> {
    let pixel = vec2<i32>(floor(input.position.xy));
    let under = textureLoad(picture, pixel, 0);
    let over = textureLoad(overlay, pixel, 0);
    let front = clamp(over.a, 0.0, 1.0);
    if data.flags.x < 0.5 {
        let encoded = clamp(over.rgb + under.rgb * (1.0 - front), vec3<f32>(0.0), vec3<f32>(1.0));
        return vec4<f32>(encoded, 1.0);
    }
    let back = clamp(under.a, 0.0, 1.0);
    let covered = back * (1.0 - front);
    let alpha = front + covered;
    if alpha <= 0.0 {
        return vec4<f32>(0.0);
    }
    var color = srgb_decode(under.rgb) * covered;
    if front > 0.0 {
        color += srgb_decode(over.rgb / front) * front;
    }
    return vec4<f32>(srgb_encode(color / alpha), alpha);
}
