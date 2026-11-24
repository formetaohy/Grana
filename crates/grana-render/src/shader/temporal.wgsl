struct TemporalData {
    previous: mat4x4<f32>,
    eye: vec4<f32>,
    flags: vec4<f32>,
    depth: vec4<f32>,
    quantum: vec4<f32>,
}

@group(2) @binding(0) var<uniform> temporal: TemporalData;
@group(2) @binding(1) var current: texture_2d<f32>;
@group(2) @binding(2) var history_color: texture_2d<f32>;
@group(2) @binding(3) var history_statistics: texture_2d<f32>;
@group(2) @binding(4) var history_sampler: sampler;

const UNREACHED: f32 = 1.0e19;

const UNKNOWN: f32 = 1.0e18;

const DEPTH_MARGIN: f32 = 6.0;

const NO_SURFACE: f32 = 1.0;

const NO_SURFACE_EDGE: f32 = 0.995;

const ENCODED_CEILING: f32 = 0.99;

const LUMA: vec3<f32> = vec3<f32>(0.2126, 0.7152, 0.0722);

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
    @location(2) statistics: vec4<f32>,
}

struct Range {
    low: vec4<f32>,
    high: vec4<f32>,
    luma: vec2<f32>,
}

struct Reprojection {
    uv: vec2<f32>,
    valid: bool,
}

struct Specimen {
    world: vec3<f32>,
    known: bool,
    reach: f32,
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
    var range = Range(vec4<f32>(1.0e20), vec4<f32>(-1.0e20), vec2<f32>(1.0e20, -1.0e20));
    for (var y = -1; y <= 1; y += 1) {
        for (var x = -1; x <= 1; x += 1) {
            let value = textureLoad(current, clamp(at + vec2<i32>(x, y), low, high), 0);
            let chroma = to_ycocg(value.rgb);
            range.low = min(range.low, vec4<f32>(chroma, value.a));
            range.high = max(range.high, vec4<f32>(chroma, value.a));
            let luma = dot(value.rgb, LUMA);
            range.luma = vec2<f32>(min(range.luma.x, luma), max(range.luma.y, luma));
        }
    }
    return range;
}

fn clamp_history(stored: vec4<f32>, range: Range) -> vec4<f32> {
    let chroma = clamp(to_ycocg(stored.rgb), range.low.rgb, range.high.rgb);
    return vec4<f32>(from_ycocg(chroma), clamp(stored.a, range.low.a, range.high.a));
}

fn specimen(pixel: vec2<i32>, ndc: vec2<f32>) -> Specimen {
    let surface = motion_surface(pixel, ndc);
    let world = unproject(vec3<f32>(ndc, surface.depth));
    let reach = select(UNREACHED, length(world - temporal.eye.xyz), surface.known);
    return Specimen(world, surface.known, reach);
}

fn encode_reach(held: Specimen) -> f32 {
    if (!held.known) {
        return NO_SURFACE;
    }
    return clamp(0.5 * held.reach * temporal.depth.y, 0.0, ENCODED_CEILING);
}

fn survives(encoded: f32, world: Specimen) -> bool {
    if (!world.known) {
        return encoded >= NO_SURFACE_EDGE;
    }
    if (encoded >= NO_SURFACE_EDGE) {
        return false;
    }
    let held = encoded * 2.0 * temporal.depth.z;
    let margin = max(DEPTH_MARGIN * pixel_world(world.reach), temporal.quantum.x);
    return abs(held - world.reach) <= margin;
}

fn reproject(world: vec3<f32>, extent: vec2<f32>) -> Reprojection {
    let clip = temporal.previous * vec4<f32>(world, 1.0);
    let previous = clip.xy / clip.w;
    let local = vec2<f32>(previous.x * 0.5 + 0.5, 0.5 - previous.y * 0.5);
    let uv = (frame.viewport.xy + local * frame.viewport.zw) / extent;
    let reach = frame.viewport.xy + local * frame.viewport.zw;
    let valid = reach.x >= frame.viewport.x - 0.5
        && reach.y >= frame.viewport.y - 0.5
        && reach.x <= frame.viewport.x + frame.viewport.z - 0.5
        && reach.y <= frame.viewport.y + frame.viewport.w - 0.5;
    return Reprojection(uv, valid);
}

fn fresh(color: vec4<f32>, world: Specimen) -> TemporalOutput {
    return TemporalOutput(
        color,
        color,
        vec4<f32>(2.0, 1.0, dot(color.rgb, LUMA), encode_reach(world)),
    );
}

@fragment
fn fs_main(input: VertexOutput) -> TemporalOutput {
    let pixel = vec2<i32>(input.clip.xy);
    let extent = vec2<f32>(textureDimensions(current));
    let origin = vec2<i32>(frame.viewport.xy);
    let last = origin + vec2<i32>(frame.viewport.zw) - vec2<i32>(1);
    let local = (input.clip.xy - frame.viewport.xy) / frame.viewport.zw;
    let ndc = vec2<f32>(local.x * 2.0 - 1.0, 1.0 - local.y * 2.0);
    let colour = textureLoad(current, pixel, 0);
    let world = specimen(pixel, ndc);
    if (temporal.flags.x <= 0.5) {
        return fresh(colour, world);
    }
    let projection = reproject(world.world, extent);
    if (!projection.valid) {
        return fresh(colour, world);
    }
    let at = clamp(vec2<i32>(projection.uv * extent), origin, last);
    let stored = textureLoad(history_statistics, pixel, 0);
    if (temporal.flags.y > 0.5 && !survives(textureLoad(history_statistics, at, 0).w, world)) {
        return fresh(colour, world);
    }
    var held = textureSampleLevel(history_color, history_sampler, projection.uv, 0.0);
    var odd = stored.z;
    if (temporal.flags.y > 0.5) {
        let range = neighbourhood(pixel, origin, last);
        held = clamp_history(held, range);
        odd = clamp(odd, range.luma.x, range.luma.y);
    }
    let carried = clamp(stored.x, 2.0, temporal.flags.z - 1.0);
    let odd_carried = clamp(stored.y, 1.0, carried);
    let index = carried + 1.0;
    let sample_odd = sample_noise(vec2<f32>(pixel), index) < 0.5;
    let resolved = mix(held, colour, 1.0 / index);
    var next_odd = odd;
    var odd_count = odd_carried;
    if (sample_odd) {
        next_odd = mix(odd, dot(colour.rgb, LUMA), 1.0 / (odd_carried + 1.0));
        odd_count = odd_carried + 1.0;
    }
    return TemporalOutput(
        resolved,
        resolved,
        vec4<f32>(index, odd_count, next_odd, encode_reach(world)),
    );
}
