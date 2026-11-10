@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var reference: texture_2d<f32>;

const COVERAGE: f32 = 0.01;
const MINIMUM_ALPHA: f32 = 1.0e-4;
const DEVIATION_LIMIT: f32 = 8.0;

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
fn fs_error(input: VertexOutput) -> @location(0) vec4<f32> {
    let pixel = vec2<i32>(input.clip_position.xy);
    let sample = textureLoad(source, pixel, 0);
    let sum = textureLoad(reference, pixel, 0);
    if (sum.a <= COVERAGE) {
        return vec4<f32>(0.0);
    }
    let straight = sample.rgb / max(sample.a, MINIMUM_ALPHA);
    let mean = sum.rgb / sum.a;
    let delta = max(
        abs(straight.r - mean.r),
        max(abs(straight.g - mean.g), abs(straight.b - mean.b)),
    );
    return vec4<f32>(min(delta, DEVIATION_LIMIT), 1.0, 0.0, 0.0);
}

@fragment
fn fs_reduce(input: VertexOutput) -> @location(0) vec4<f32> {
    let size = vec2<i32>(textureDimensions(source));
    let base = vec2<i32>(input.clip_position.xy) * 2;
    var total = vec4<f32>(0.0);
    for (var y = 0; y < 2; y = y + 1) {
        for (var x = 0; x < 2; x = x + 1) {
            let position = clamp(base + vec2<i32>(x, y), vec2<i32>(0), size - vec2<i32>(1));
            total += textureLoad(source, position, 0);
        }
    }
    return total * 0.25;
}
