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
    let texel = 1.0 / vec2<f32>(textureDimensions(shadow_objects));
    let reference = coordinate.z - bias;
    let offsets = array<vec2<f32>, 4>(
        vec2<f32>(0.5, 0.5),
        vec2<f32>(-0.5, 0.5),
        vec2<f32>(0.5, -0.5),
        vec2<f32>(-0.5, -0.5),
    );
    var visibility = 0.0;
    for (var tap = 0u; tap < 4u; tap += 1u) {
        visibility += textureSampleCompareLevel(
            shadow_objects,
            shadow_compare,
            coordinate.xy + offsets[tap] * texel,
            reference,
        );
    }
    return 0.25 * visibility;
}

fn shadow_bias(reach: vec4<f32>, normal: vec3<f32>, light: vec3<f32>) -> f32 {
    let cosine = max(dot(normal, light), 0.08);
    let tangent = sqrt(max(1.0 - cosine * cosine, 0.0)) / cosine;
    return reach.y + reach.w * min(tangent, 12.0);
}

fn sun_transmittance(coordinate: vec3<f32>, bias: f32) -> f32 {
    return tau_transmittance(coordinate) * object_visibility(coordinate, bias);
}

fn local_occlusion(uvw: vec3<f32>) -> f32 {
    return textureSampleLevel(
        shadow_occlusion,
        shadow_sampler,
        clamp(uvw, vec3<f32>(0.0), vec3<f32>(1.0)),
        0.0,
    ).r;
}
