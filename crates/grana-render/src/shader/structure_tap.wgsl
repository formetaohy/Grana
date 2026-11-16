@group(2) @binding(2) var structure_volume: texture_3d<f32>;

fn structure_tap(texel: vec3<f32>) -> vec4<f32> {
    return textureSampleLevel(structure_volume, atlas_sampler, texel, 0.0);
}

fn structure_normal(texel: vec3<f32>) -> vec3<f32> {
    let packed = structure_tap(texel);
    if (packed.z <= 0.0) {
        return vec3<f32>(0.0);
    }
    return octahedral_decode(packed.xy * 2.0 - vec2<f32>(1.0));
}
