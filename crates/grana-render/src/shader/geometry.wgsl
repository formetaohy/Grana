const UNREACHED_DEPTH: f32 = 1.0;
const NORMAL_REACH: f32 = 0.5;
const SURFACE_SCALE: i32 = 2;

struct Surface {
    depth: f32,
    normal: vec3<f32>,
    known: bool,
}

fn empty_surface() -> Surface {
    return Surface(UNREACHED_DEPTH, vec3<f32>(0.0), false);
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

fn geometry_of(surface: Surface) -> vec4<f32> {
    return vec4<f32>(surface.normal, surface.depth);
}

fn surface_of(geometry: vec4<f32>) -> Surface {
    return Surface(geometry.w, geometry.xyz, geometry.w < UNREACHED_DEPTH);
}
