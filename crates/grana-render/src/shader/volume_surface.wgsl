@group(1) @binding(2) var volume_surface: texture_2d<f32>;
@group(1) @binding(3) var volume_normal: texture_2d<f32>;

fn volume_texel(pixel: vec2<i32>) -> vec2<i32> {
    let extent = vec2<i32>(textureDimensions(volume_surface));
    return clamp(pixel, vec2<i32>(0), extent - vec2<i32>(1));
}

fn volume_surface_at(pixel: vec2<i32>) -> Surface {
    let at = volume_texel(pixel);
    let depth = textureLoad(volume_surface, at, 0).y;
    if (depth >= UNREACHED_DEPTH) {
        return empty_surface();
    }
    let normal = textureLoad(volume_normal, at, 0);
    if (normal.w < NORMAL_REACH) {
        return Surface(depth, vec3<f32>(0.0), true);
    }
    return Surface(depth, normal.xyz, true);
}

fn volume_motion_at(pixel: vec2<i32>) -> Surface {
    let surface = volume_surface_at(pixel);
    if (surface.known) {
        return surface;
    }
    let front = textureLoad(volume_surface, volume_texel(pixel), 0).x;
    if (front >= UNREACHED_DEPTH) {
        return surface;
    }
    return Surface(front, vec3<f32>(0.0), true);
}
