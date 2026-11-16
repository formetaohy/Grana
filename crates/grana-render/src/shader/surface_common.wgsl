struct ResolvedSurface {
    @location(0) value: vec4<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let corners = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    return vec4<f32>(corners[index], 0.0, 1.0);
}

fn nearest_object(base: vec2<i32>) -> Surface {
    var chosen = empty_surface();
    for (var y = 0; y < SURFACE_SCALE; y += 1) {
        for (var x = 0; x < SURFACE_SCALE; x += 1) {
            let sample = object_surface(base + vec2<i32>(x, y));
            if (sample.known && (!chosen.known || sample.depth < chosen.depth)) {
                chosen = sample;
            }
        }
    }
    return chosen;
}

fn surface_value(surface: Surface) -> vec4<f32> {
    if (!surface.known) {
        return vec4<f32>(0.0, 0.0, UNREACHED_DEPTH, 0.0);
    }
    if (length(surface.normal) < NORMAL_REACH) {
        return vec4<f32>(0.0, 0.0, surface.depth, 0.0);
    }
    return vec4<f32>(octahedral_encode(normalize(surface.normal)), surface.depth, 1.0);
}
