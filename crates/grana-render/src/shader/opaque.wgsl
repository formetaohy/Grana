@group(6) @binding(0) var opaque_depth: texture_depth_2d;

fn behind_opaque(pixel: vec2<i32>, depth: f32) -> bool {
    let extent = vec2<i32>(textureDimensions(opaque_depth));
    let at = clamp(pixel, vec2<i32>(0), extent - vec2<i32>(1));
    return depth >= textureLoad(opaque_depth, at, 0);
}
