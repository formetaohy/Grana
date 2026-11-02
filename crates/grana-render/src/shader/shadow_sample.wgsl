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
