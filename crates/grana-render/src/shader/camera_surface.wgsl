fn slice_surface(ndc: vec2<f32>) -> Surface {
    let ray = camera_ray(ndc);
    let crossing = slice_crossing(ray.origin, ray.direction);
    if (crossing.w < 0.5) {
        return empty_surface();
    }
    return Surface(clip_depth(crossing.xyz), vec3<f32>(0.0), true);
}

fn motion_surface(pixel: vec2<i32>, ndc: vec2<f32>) -> Surface {
    let object = object_surface(pixel);
    if (slice_active()) {
        return nearest_surface(object, slice_surface(ndc));
    }
    return nearest_surface(object, volume_motion_at(pixel));
}
