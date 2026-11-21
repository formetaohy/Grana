struct Limit {
    region: vec4<u32>,
}

@group(0) @binding(0) var<uniform> limit: Limit;
@group(0) @binding(1) var statistics: texture_2d<f32>;
@group(0) @binding(2) var resolved: texture_2d<f32>;
@group(0) @binding(3) var<storage, read_write> worst: array<atomic<u32>, 2>;

const LUMINANCE_FLOOR: f32 = 1.0e-3;

fn luminance(color: vec3<f32>) -> f32 {
    return dot(color, vec3<f32>(0.2126, 0.7152, 0.0722));
}

@compute @workgroup_size(8, 8)
fn reduce(@builtin(global_invocation_id) id: vec3<u32>) {
    let at = limit.region.xy + id.xy;
    let far = limit.region.xy + limit.region.zw;
    if (at.x >= far.x || at.y >= far.y) {
        return;
    }
    let pixel = vec2<i32>(at);
    let carried = textureLoad(statistics, pixel, 0);
    let count = carried.x;
    var error = 1.0;
    if (count >= 2.0) {
        let deviation = sqrt(max(carried.y / (count - 1.0), 0.0) / count);
        let level = max(luminance(textureLoad(resolved, pixel, 0).rgb), LUMINANCE_FLOOR);
        error = min(deviation / level, 1.0);
    }
    atomicMax(&worst[0], bitcast<u32>(error));
    atomicMin(&worst[1], bitcast<u32>(count));
}
