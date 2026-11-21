struct Limit {
    region: vec4<u32>,
    exposure: vec4<f32>,
}

@group(0) @binding(0) var<uniform> limit: Limit;
@group(0) @binding(1) var current: texture_2d<f32>;
@group(0) @binding(2) var previous: texture_2d<f32>;
@group(0) @binding(3) var<storage, read_write> bins: array<atomic<u32>, 64>;

const BINS: u32 = 64u;
const CEILING: f32 = 0.05;
const TOP: u32 = 63u;
const COVERAGE_FLOOR: f32 = 0.05;

fn luminance(color: vec3<f32>) -> f32 {
    return dot(color, vec3<f32>(0.2126, 0.7152, 0.0722));
}

fn presented(sample: vec4<f32>) -> f32 {
    let coverage = clamp(sample.a, 0.0, 1.0);
    let straight = max(luminance(sample.rgb), 0.0) / max(coverage, 1.0e-4);
    return srgb_encode(aces(vec3<f32>(straight * limit.exposure.x)) * coverage).x;
}

@compute @workgroup_size(8, 8)
fn reduce(@builtin(global_invocation_id) id: vec3<u32>) {
    let at = limit.region.xy + id.xy;
    let far = limit.region.xy + limit.region.zw;
    if (at.x >= far.x || at.y >= far.y) {
        return;
    }
    let pixel = vec2<i32>(at);
    let sample = textureLoad(current, pixel, 0);
    if (clamp(sample.a, 0.0, 1.0) < COVERAGE_FLOOR) {
        return;
    }
    let change = abs(presented(sample) - presented(textureLoad(previous, pixel, 0)));
    atomicAdd(&bins[min(u32(change / CEILING * f32(BINS)), TOP)], 1u);
}
