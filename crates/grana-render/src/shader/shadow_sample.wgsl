const SHADOW_TAPS: u32 = 4u;
const SHADOW_ROTATION: f32 = 0.29452431;
const SHADOW_SPREAD: f32 = 0.75;

fn shadow_coordinate(view: mat4x4<f32>, world: vec3<f32>) -> vec3<f32> {
    return (view * vec4<f32>(world, 1.0)).xyz;
}

fn tau_transmittance(coordinate: vec3<f32>) -> f32 {
    if (any(coordinate < vec3<f32>(0.0)) || any(coordinate > vec3<f32>(1.0))) {
        return 1.0;
    }
    return exp(-decoded_tau(textureSampleLevel(shadow_tau, shadow_sampler, coordinate, 0.0).r));
}

fn object_visibility(coordinate: vec3<f32>, bias: f32) -> f32 {
    if (any(coordinate < vec3<f32>(0.0)) || any(coordinate > vec3<f32>(1.0))) {
        return 1.0;
    }
    let texel = SHADOW_SPREAD / f32(textureDimensions(shadow_objects).x);
    let reference = coordinate.z - bias;
    var visibility = 0.0;
    for (var tap = 0u; tap < SHADOW_TAPS; tap += 1u) {
        let angle = SHADOW_ROTATION + f32(tap) * 1.5707963;
        let offset = vec2<f32>(cos(angle), sin(angle)) * texel;
        visibility += textureSampleCompareLevel(
            shadow_objects,
            shadow_compare,
            coordinate.xy + offset,
            reference,
        );
    }
    return visibility / f32(SHADOW_TAPS);
}

fn shadow_bias(reach: vec4<f32>, normal: vec3<f32>, light: vec3<f32>) -> f32 {
    let cosine = max(dot(normal, light), 0.08);
    let tangent = sqrt(max(1.0 - cosine * cosine, 0.0)) / cosine;
    return reach.y + reach.w * min(tangent, 12.0);
}

fn sun_transmittance(coordinate: vec3<f32>, bias: f32) -> f32 {
    return tau_transmittance(coordinate) * object_visibility(coordinate, bias);
}

