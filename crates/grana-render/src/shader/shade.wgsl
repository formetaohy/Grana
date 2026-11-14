@group(2) @binding(2) var structure_volume: texture_3d<f32>;
@group(6) @binding(0) var shadow_tau: texture_3d<f32>;
@group(6) @binding(1) var shadow_sampler: sampler;
@group(6) @binding(3) var shadow_objects: texture_depth_2d;
@group(6) @binding(4) var shadow_compare: sampler_comparison;

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

fn volume_lighting(albedo: vec3<f32>, texel: vec3<f32>, world: vec3<f32>) -> vec3<f32> {
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
    let strength = clamp(frame.march.y, 0.0, 1.0);
    var visibility = 1.0;
    if (strength > 0.0) {
        let lifted = world + normal * frame.light_reach.z;
        let sun = sun_transmittance(
            shadow_coordinate(frame.light_view, lifted),
            shadow_bias(frame.light_reach, normal, frame.light.xyz),
        );
        visibility = mix(1.0, sun, strength);
    }
    let cosine = max(dot(normal, frame.light.xyz), 0.0);
    let lit = albedo
        * (frame.shading.x * scene_environment(normal) + frame.shading.y * cosine * visibility);
    return mix(albedo, lit, clamp(structure.z, 0.0, 1.0));
}
