@group(1) @binding(0) var object_depth: texture_depth_2d;
@group(1) @binding(1) var object_normal: texture_2d<f32>;

fn object_surface(pixel: vec2<i32>) -> Surface {
    let depth = textureLoad(object_depth, pixel, 0);
    if (depth >= UNREACHED_DEPTH) {
        return empty_surface();
    }
    return Surface(depth, octahedral_decode(textureLoad(object_normal, pixel, 0).xy), true);
}
