@group(2) @binding(2) var structure_volume: texture_3d<f32>;

const STRUCTURE_PAD: f32 = 1.0;

fn structure_tap(texel: vec3<f32>) -> vec4<f32> {
    let grid = vec3<f32>(frame.atlas.xyz);
    let dims = vec3<f32>(textureDimensions(structure_volume));
    let side = dims / grid;
    let position = clamp(texel, vec3<f32>(0.0), vec3<f32>(1.0)) * grid * PADDED_SIDE;
    let cell = clamp(floor(position / PADDED_SIDE), vec3<f32>(0.0), grid - vec3<f32>(1.0));
    let local = (position - cell * PADDED_SIDE) * 0.5 + vec3<f32>(STRUCTURE_PAD);
    return textureSampleLevel(
        structure_volume,
        atlas_sampler,
        (cell * side + local) / dims,
        0.0,
    );
}

fn structure_sample_normal(structure: vec4<f32>) -> vec4<f32> {
    if (structure.z <= 0.0) {
        return vec4<f32>(0.0);
    }
    return vec4<f32>(octahedral_decode(structure.xy * 2.0 - vec2<f32>(1.0)), 1.0);
}
