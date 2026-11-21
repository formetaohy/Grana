struct Limit {
    region: vec4<u32>,
    scale: vec4<f32>,
}

@group(0) @binding(0) var<uniform> limit: Limit;
@group(0) @binding(1) var resolved: texture_2d<f32>;
@group(0) @binding(2) var statistics: texture_2d<f32>;
@group(0) @binding(3) var<storage, read_write> bins: array<atomic<u32>, 64>;
@group(0) @binding(4) var odd: texture_2d<f32>;

const BINS: u32 = 64u;
const TOP: u32 = 63u;
const COVERAGE_FLOOR: f32 = 0.05;
const LEVELS: f32 = 255.0;

fn presented(sample: vec4<f32>) -> f32 {
    let coverage = clamp(sample.a, 0.0, 1.0);
    let straight = max(dot(sample.rgb, vec3<f32>(0.2126, 0.7152, 0.0722)), 0.0) / max(coverage, 1.0e-4);
    return srgb_encode(aces(vec3<f32>(straight * limit.scale.w)) * coverage).x;
}

fn bin_of(error: f32) -> u32 {
    let fraction = clamp(sqrt(max(error, 0.0) / limit.scale.x), 0.0, 1.0);
    return min(u32(fraction * f32(BINS)), TOP);
}

@compute @workgroup_size(8, 8)
fn reduce(@builtin(global_invocation_id) id: vec3<u32>) {
    let at = limit.region.xy + id.xy;
    let far = limit.region.xy + limit.region.zw;
    if (at.x >= far.x || at.y >= far.y) {
        return;
    }
    let pixel = vec2<i32>(at);
    if (clamp(textureLoad(resolved, pixel, 0).a, 0.0, 1.0) < COVERAGE_FLOOR) {
        return;
    }
    let carried = textureLoad(statistics, pixel, 0);
    var bin = TOP;
    if (carried.x >= limit.scale.z) {
        let error = LEVELS * limit.scale.y * abs(presented(textureLoad(resolved, pixel, 0)) - presented(textureLoad(odd, pixel, 0)));
        if (error >= 0.0) {
            bin = bin_of(error);
        }
    }
    atomicAdd(&bins[bin], 1u);
}
