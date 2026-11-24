struct Limit {
    region: vec4<u32>,
    scale: vec4<f32>,
}

@group(0) @binding(0) var<uniform> limit: Limit;
@group(0) @binding(1) var resolved: texture_2d<f32>;
@group(0) @binding(2) var statistics: texture_2d<f32>;
@group(0) @binding(3) var<storage, read_write> bins: array<atomic<u32>, 64>;

const BINS: u32 = 64u;
const TOP: u32 = 63u;
const COVERAGE_FLOOR: f32 = 0.05;
const LEVELS: f32 = 255.0;
const WEIGHT: f32 = 255.0;
const TILE: i32 = 2;
const LUMA: vec3<f32> = vec3<f32>(0.2126, 0.7152, 0.0722);

fn presented_luma(luma: f32, coverage: f32) -> f32 {
    let straight = max(luma, 0.0) / max(coverage, 1.0e-4);
    return srgb_encode(aces(vec3<f32>(straight * limit.scale.w)) * coverage).x;
}

fn bin_of(error: f32) -> u32 {
    let fraction = clamp(sqrt(max(error, 0.0) / limit.scale.x), 0.0, 1.0);
    return min(u32(fraction * f32(BINS)), TOP);
}

struct Tile {
    resolved: f32,
    odd: f32,
}

fn tile_luminance(pixel: vec2<i32>) -> Tile {
    let origin = limit.region.xy;
    let far = origin + limit.region.zw - vec2<u32>(1u);
    let base = pixel & vec2<i32>(-2);
    var sum = Tile(0.0, 0.0);
    for (var y = 0; y < TILE; y += 1) {
        for (var x = 0; x < TILE; x += 1) {
            let at = clamp(base + vec2<i32>(x, y), vec2<i32>(origin), vec2<i32>(far));
            let sample = textureLoad(resolved, at, 0);
            let coverage = clamp(sample.a, 0.0, 1.0);
            sum.resolved += presented_luma(dot(sample.rgb, LUMA), coverage);
            sum.odd += presented_luma(textureLoad(statistics, at, 0).z, coverage);
        }
    }
    return Tile(sum.resolved / f32(TILE * TILE), sum.odd / f32(TILE * TILE));
}

@compute @workgroup_size(8, 8)
fn reduce(@builtin(global_invocation_id) id: vec3<u32>) {
    let at = limit.region.xy + id.xy;
    let far = limit.region.xy + limit.region.zw;
    if (at.x >= far.x || at.y >= far.y) {
        return;
    }
    let pixel = vec2<i32>(at);
    let coverage = clamp(textureLoad(resolved, pixel, 0).a, 0.0, 1.0);
    if (coverage < COVERAGE_FLOOR) {
        return;
    }
    let carried = textureLoad(statistics, pixel, 0);
    var bin = TOP;
    if (carried.x >= limit.scale.z) {
        let tile = tile_luminance(pixel);
        bin = bin_of(LEVELS * abs(tile.resolved - tile.odd));
    }
    let weight = max(u32(coverage * WEIGHT + 0.5), 1u);
    atomicAdd(&bins[bin], weight);
}
