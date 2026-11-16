@group(1) @binding(0) var scene_surface: texture_2d<f32>;

fn surface_texel(at: vec2<i32>) -> vec2<i32> {
    let extent = vec2<i32>(textureDimensions(scene_surface));
    return clamp(at, vec2<i32>(0), extent - vec2<i32>(1));
}

fn surface_at(at: vec2<i32>) -> Surface {
    let value = textureLoad(scene_surface, surface_texel(at), 0);
    if (value.z >= UNREACHED_DEPTH) {
        return empty_surface();
    }
    if (value.w < NORMAL_REACH) {
        return Surface(value.z, vec3<f32>(0.0), true);
    }
    return Surface(value.z, octahedral_decode(value.xy), true);
}

fn surface_depth(at: vec2<i32>) -> f32 {
    return textureLoad(scene_surface, surface_texel(at), 0).z;
}
