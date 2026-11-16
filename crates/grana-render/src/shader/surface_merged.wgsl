@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> ResolvedSurface {
    let base = vec2<i32>(floor(position.xy)) * SURFACE_SCALE;
    let object = nearest_object(base);
    return ResolvedSurface(surface_value(nearest_surface(object, volume_surface_at(base))));
}
