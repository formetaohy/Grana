fn octahedral_sign(axis: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(select(-1.0, 1.0, axis.x >= 0.0), select(-1.0, 1.0, axis.y >= 0.0));
}

fn octahedral_encode(normal: vec3<f32>) -> vec2<f32> {
    let unit = normal / max(abs(normal.x) + abs(normal.y) + abs(normal.z), 1.0e-8);
    if (unit.z >= 0.0) {
        return unit.xy;
    }
    return (vec2<f32>(1.0) - abs(unit.yx)) * octahedral_sign(unit.xy);
}

fn octahedral_decode(packed: vec2<f32>) -> vec3<f32> {
    let z = 1.0 - abs(packed.x) - abs(packed.y);
    var unit = vec3<f32>(packed, z);
    if (z < 0.0) {
        unit = vec3<f32>((vec2<f32>(1.0) - abs(packed.yx)) * octahedral_sign(packed), z);
    }
    if (dot(unit, unit) <= 0.0) {
        return vec3<f32>(0.0);
    }
    return normalize(unit);
}
