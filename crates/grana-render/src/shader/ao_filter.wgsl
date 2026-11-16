@group(0) @binding(1) var filtered_ao: texture_2d<f32>;

const MAX_SLOPE: f32 = 8.0;

fn tap_world(at: vec2<i32>) -> vec4<f32> {
    let extent = vec2<i32>(ao.extent.xy);
    let pixel = clamp(at * AO_SCALE, vec2<i32>(0), extent - vec2<i32>(1));
    let depth = visible_depth(pixel);
    if (depth >= UNREACHED_DEPTH) {
        return vec4<f32>(0.0);
    }
    return vec4<f32>(world_of(region_ndc(vec2<f32>(pixel), ao.region), depth), 1.0);
}

@fragment
fn fs_filter(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let pixel = vec2<i32>(position.xy);
    let surface = visible_surface(pixel);
    if (!surface.known || length(surface.normal) < NORMAL_REACH) {
        return vec4<f32>(1.0);
    }
    let world = world_of(region_ndc(vec2<f32>(pixel), ao.region), surface.depth);
    let view = normalize(ao.eye.xyz - world);
    let facing = max(abs(dot(normalize(surface.normal), view)), FLOOR);
    let span = f32(AO_SCALE) * pixel_world(world);
    let tolerance = clamp(span / facing, span, span * MAX_SLOPE);
    let extent = vec2<i32>(textureDimensions(filtered_ao));
    let base = (pixel - vec2<i32>(1)) / AO_SCALE;
    var sum = 0.0;
    var weight = 0.0;
    for (var y = 0; y < 2; y += 1) {
        for (var x = 0; x < 2; x += 1) {
            let at = clamp(
                base + vec2<i32>(x, y),
                vec2<i32>(0),
                extent - vec2<i32>(1),
            );
            let tap = tap_world(at);
            if (tap.w < 0.5) {
                continue;
            }
            let reach = 1.0 - clamp(distance(tap.xyz, world) / tolerance, 0.0, 1.0);
            sum += reach * textureLoad(filtered_ao, at, 0).r;
            weight += reach;
        }
    }
    if (weight <= 0.0) {
        return vec4<f32>(textureLoad(filtered_ao, pixel / AO_SCALE, 0).r);
    }
    return vec4<f32>(sum / weight);
}
