@group(0) @binding(1) var filtered_ao: texture_2d<f32>;
@group(0) @binding(2) var filter_depth: texture_depth_2d;
@group(0) @binding(3) var filter_normal: texture_2d<f32>;

const MAX_SLOPE: f32 = 8.0;

fn tap_world(at: vec2<i32>) -> vec4<f32> {
    let extent = vec2<i32>(ao.extent.xy);
    let pixel = clamp(at, vec2<i32>(0), extent - vec2<i32>(1));
    let depth = textureLoad(filter_depth, pixel, 0);
    if (depth >= 1.0) {
        return vec4<f32>(0.0);
    }
    return vec4<f32>(world_of(ndc_of(vec2<f32>(pixel)), depth), 1.0);
}

@fragment
fn fs_filter(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let pixel = vec2<i32>(position.xy);
    let depth = textureLoad(filter_depth, pixel, 0);
    if (depth >= 1.0) {
        return vec4<f32>(1.0);
    }
    let world = world_of(ndc_of(vec2<f32>(pixel)), depth);
    let view = normalize(ao.eye.xyz - world);
    let normal = octahedral_decode(textureLoad(filter_normal, pixel, 0).xy);
    let facing = max(abs(dot(normalize(normal), view)), FLOOR);
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
            let tap = tap_world(at * AO_SCALE);
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
