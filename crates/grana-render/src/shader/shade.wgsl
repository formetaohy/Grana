@group(6) @binding(0) var shadow_tau: texture_3d<f32>;
@group(6) @binding(1) var shadow_sampler: sampler;
@group(6) @binding(3) var shadow_objects: texture_depth_2d;
@group(6) @binding(4) var shadow_compare: sampler_comparison;

fn structure_lighting(albedo: vec3<f32>, texel: vec3<f32>, world: vec3<f32>) -> vec3<f32> {
    let structure = structure_tap(texel);
    if (structure.z <= 0.0) {
        return albedo;
    }
    var normal = octahedral_decode(structure.xy * 2.0 - vec2<f32>(1.0));
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
