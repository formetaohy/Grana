@group(2) @binding(2) var structure_volume: texture_3d<f32>;

fn sign_not_zero(axis: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(select(-1.0, 1.0, axis.x >= 0.0), select(-1.0, 1.0, axis.y >= 0.0));
}

fn octahedral_decode(packed: vec2<f32>) -> vec3<f32> {
    var normal = vec3<f32>(packed, 1.0 - abs(packed.x) - abs(packed.y));
    if (normal.z < 0.0) {
        normal = vec3<f32>((vec2<f32>(1.0) - abs(normal.yx)) * sign_not_zero(normal.xy), normal.z);
    }
    if (dot(normal, normal) <= 0.0) {
        return vec3<f32>(0.0);
    }
    return normalize(normal);
}

fn structure_sample(texel: vec3<f32>) -> vec4<f32> {
    return textureSampleLevel(structure_volume, atlas_sampler, texel, 0.0);
}

fn structure_normal(texel: vec3<f32>) -> vec3<f32> {
    let structure = structure_sample(texel);
    if (structure.z <= 0.0) {
        return vec3<f32>(0.0);
    }
    return octahedral_decode(structure.xy);
}

fn volume_lighting(albedo: vec3<f32>, texel: vec3<f32>) -> vec3<f32> {
    let structure = structure_sample(texel);
    if (structure.z <= 0.0) {
        return albedo;
    }
    var normal = octahedral_decode(structure.xy);
    if (dot(normal, normal) <= 0.0) {
        return albedo;
    }
    if (dot(normal, frame.light.xyz) < 0.0) {
        normal = -normal;
    }
    let cosine = max(dot(normal, frame.light.xyz), 0.0);
    let lit = albedo
        * (frame.shading.x * scene_environment(normal) + frame.shading.y * cosine);
    return mix(albedo, lit, clamp(structure.z, 0.0, 1.0));
}
