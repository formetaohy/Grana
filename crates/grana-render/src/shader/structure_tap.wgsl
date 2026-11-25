@group(2) @binding(2) var structure_volume: texture_3d<f32>;

fn structure_tap(tap: BrickTap, local: vec3<f32>) -> vec4<f32> {
    let side = textureDimensions(structure_volume) / frame.atlas.xyz;
    let origin = slot_cell(tap.slot) * side;
    let position = structure_position(local);
    let low = origin + vec3<u32>(floor(position));
    let weight = position - floor(position);
    let x00 = textureLoad(structure_volume, low, 0);
    let x10 = textureLoad(structure_volume, low + vec3<u32>(1u, 0u, 0u), 0);
    let x01 = textureLoad(structure_volume, low + vec3<u32>(0u, 1u, 0u), 0);
    let x11 = textureLoad(structure_volume, low + vec3<u32>(1u, 1u, 0u), 0);
    let y00 = textureLoad(structure_volume, low + vec3<u32>(0u, 0u, 1u), 0);
    let y10 = textureLoad(structure_volume, low + vec3<u32>(1u, 0u, 1u), 0);
    let y01 = textureLoad(structure_volume, low + vec3<u32>(0u, 1u, 1u), 0);
    let y11 = textureLoad(structure_volume, low + vec3<u32>(1u, 1u, 1u), 0);
    let near = mix(mix(x00, x10, weight.x), mix(x01, x11, weight.x), weight.y);
    let far = mix(mix(y00, y10, weight.x), mix(y01, y11, weight.x), weight.y);
    return mix(near, far, weight.z);
}

fn structure_sample_normal(structure: vec4<f32>) -> vec4<f32> {
    if (structure.z <= 0.0) {
        return vec4<f32>(0.0);
    }
    return vec4<f32>(octahedral_decode(structure.xy * 2.0 - vec2<f32>(1.0)), 1.0);
}
