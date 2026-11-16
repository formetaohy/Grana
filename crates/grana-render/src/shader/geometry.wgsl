@group(1) @binding(0) var object_depth: texture_depth_2d;
@group(1) @binding(1) var object_normal: texture_2d<f32>;
@group(1) @binding(2) var volume_surface: texture_2d<f32>;
@group(1) @binding(3) var volume_normal: texture_2d<f32>;

const UNREACHED_DEPTH: f32 = 1.0;
const NORMAL_REACH: f32 = 0.5;

struct Surface {
    depth: f32,
    normal: vec3<f32>,
    known: bool,
}

fn empty_surface() -> Surface {
    return Surface(UNREACHED_DEPTH, vec3<f32>(0.0), false);
}

fn region_ndc(pixel: vec2<f32>, region: vec4<f32>) -> vec2<f32> {
    let local = pixel - region.xy + vec2<f32>(0.5);
    return vec2<f32>(
        local.x / region.z * 2.0 - 1.0,
        1.0 - local.y / region.w * 2.0,
    );
}

fn object_surface(pixel: vec2<i32>) -> Surface {
    let depth = textureLoad(object_depth, pixel, 0);
    if (depth >= UNREACHED_DEPTH) {
        return empty_surface();
    }
    return Surface(depth, octahedral_decode(textureLoad(object_normal, pixel, 0).xy), true);
}

fn volume_surface_at(pixel: vec2<i32>) -> Surface {
    let at = pixel / 2;
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

fn volume_depth_at(pixel: vec2<i32>) -> f32 {
    return textureLoad(volume_surface, pixel / 2, 0).y;
}

fn nearest_surface(first: Surface, second: Surface) -> Surface {
    if (!second.known) {
        return first;
    }
    if (!first.known) {
        return second;
    }
    if (second.depth < first.depth) {
        return second;
    }
    return first;
}

fn visible_surface(pixel: vec2<i32>) -> Surface {
    return nearest_surface(object_surface(pixel), volume_surface_at(pixel));
}

fn visible_depth(pixel: vec2<i32>) -> f32 {
    return min(textureLoad(object_depth, pixel, 0), volume_depth_at(pixel));
}

fn geometry_of(surface: Surface) -> vec4<f32> {
    return vec4<f32>(surface.normal, surface.depth);
}

fn surface_of(geometry: vec4<f32>) -> Surface {
    return Surface(geometry.w, geometry.xyz, geometry.w < UNREACHED_DEPTH);
}
