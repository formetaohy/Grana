struct TemporalData {
    previous: mat4x4<f32>,
    flags: vec4<f32>,
}

@group(2) @binding(0) var<uniform> temporal: TemporalData;
@group(2) @binding(1) var current: texture_2d<f32>;
@group(2) @binding(2) var history_color: texture_2d<f32>;
@group(2) @binding(3) var history_count: texture_2d<f32>;
@group(2) @binding(4) var history_sampler: sampler;

const DRIFT: f32 = 2.0;

const QUIET: f32 = 0.015625;

struct VertexOutput {
    @builtin(position) clip: vec4<f32>,
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
    return output;
}

struct TemporalOutput {
    @location(0) retained: vec4<f32>,
    @location(1) resolved: vec4<f32>,
    @location(2) count: f32,
}

struct Range {
    low: vec4<f32>,
    high: vec4<f32>,
}

struct Reprojection {
    uv: vec2<f32>,
    drift: f32,
    valid: bool,
}

fn to_ycocg(rgb: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(
        0.25 * rgb.r + 0.5 * rgb.g + 0.25 * rgb.b,
        0.5 * rgb.r - 0.5 * rgb.b,
        -0.25 * rgb.r + 0.5 * rgb.g - 0.25 * rgb.b,
    );
}

fn from_ycocg(value: vec3<f32>) -> vec3<f32> {
    let pivot = value.x - value.z;
    return vec3<f32>(pivot + value.y, value.x + value.z, pivot - value.y);
}

fn neighbourhood(at: vec2<i32>, low: vec2<i32>, high: vec2<i32>) -> Range {
    var range = Range(vec4<f32>(1.0e20), vec4<f32>(-1.0e20));
    for (var y = -1; y <= 1; y += 1) {
        for (var x = -1; x <= 1; x += 1) {
            let value = textureLoad(current, clamp(at + vec2<i32>(x, y), low, high), 0);
            let chroma = to_ycocg(value.rgb);
            range.low = min(range.low, vec4<f32>(chroma, value.a));
            range.high = max(range.high, vec4<f32>(chroma, value.a));
        }
    }
    return range;
}

fn clamp_history(stored: vec4<f32>, range: Range) -> vec4<f32> {
    let chroma = clamp(to_ycocg(stored.rgb), range.low.rgb, range.high.rgb);
    return vec4<f32>(from_ycocg(chroma), clamp(stored.a, range.low.a, range.high.a));
}

fn reproject(pixel: vec2<i32>, ndc: vec2<f32>, extent: vec2<f32>) -> Reprojection {
    let at = input_uv(pixel, extent);
    let surface = motion_surface(pixel, ndc);
    if (!surface.known) {
        return Reprojection(at, 0.0, true);
    }
    let world = unproject(vec3<f32>(ndc, surface.depth));
    let clip = temporal.previous * vec4<f32>(world, 1.0);
    let previous = clip.xy / clip.w;
    let local = vec2<f32>(previous.x * 0.5 + 0.5, 0.5 - previous.y * 0.5);
    let uv = (frame.viewport.xy + local * frame.viewport.zw) / extent;
    let drift = length((previous - ndc) * 0.5 * frame.viewport.zw);
    let reach = frame.viewport.xy + local * frame.viewport.zw;
    let valid = reach.x >= frame.viewport.x
        && reach.y >= frame.viewport.y
        && reach.x <= frame.viewport.x + frame.viewport.z - 1.0
        && reach.y <= frame.viewport.y + frame.viewport.w - 1.0;
    return Reprojection(uv, drift, valid);
}

fn input_uv(pixel: vec2<i32>, extent: vec2<f32>) -> vec2<f32> {
    return (vec2<f32>(pixel) + vec2<f32>(0.5)) / extent;
}

fn retained(count: f32, drift: f32) -> f32 {
    let reach = clamp(DRIFT / max(drift, QUIET), 0.0, temporal.flags.z);
    return min(count, reach);
}

@fragment
fn fs_main(input: VertexOutput) -> TemporalOutput {
    let pixel = vec2<i32>(input.clip.xy);
    let extent = vec2<f32>(textureDimensions(current));
    let origin = vec2<i32>(frame.viewport.xy);
    let last = origin + vec2<i32>(frame.viewport.zw) - vec2<i32>(1);
    let local = (input.clip.xy - frame.viewport.xy) / frame.viewport.zw;
    let ndc = vec2<f32>(local.x * 2.0 - 1.0, 1.0 - local.y * 2.0);
    let color = textureLoad(current, pixel, 0);
    if (temporal.flags.x <= 0.5) {
        return TemporalOutput(color, color, 1.0);
    }
    let projection = reproject(pixel, ndc - frame.jitter.xy, extent);
    if (!projection.valid) {
        return TemporalOutput(color, color, 1.0);
    }
    let stored = textureSampleLevel(history_color, history_sampler, projection.uv, 0.0);
    let at = clamp(vec2<i32>(projection.uv * extent), origin, last);
    let held = retained(
        clamp(textureLoad(history_count, at, 0).x, 1.0, temporal.flags.z),
        projection.drift,
    );
    var history = stored;
    if (temporal.flags.y > 0.5) {
        history = clamp_history(stored, neighbourhood(pixel, origin, last));
    }
    let resolved = mix(history, color, 1.0 / (held + 1.0));
    return TemporalOutput(resolved, resolved, held + 1.0);
}
