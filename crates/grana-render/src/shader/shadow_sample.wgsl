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
    return textureSampleCompareLevel(
        shadow_objects,
        shadow_compare,
        coordinate.xy,
        coordinate.z - bias,
    );
}

fn shadow_bias(reach: vec4<f32>, normal: vec3<f32>, light: vec3<f32>) -> f32 {
    let cosine = max(dot(normal, light), 0.08);
    let tangent = sqrt(max(1.0 - cosine * cosine, 0.0)) / cosine;
    return reach.y + reach.w * min(tangent, 12.0);
}

fn sun_transmittance(coordinate: vec3<f32>, bias: f32) -> f32 {
    return tau_transmittance(coordinate) * object_visibility(coordinate, bias);
}
