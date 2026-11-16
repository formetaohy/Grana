fn ambient_occlusion(pixel: vec2<f32>) -> f32 {
    let size = vec2<i32>(textureDimensions(ao_map));
    let at = vec2<i32>(floor(pixel));
    if (at.x < 0 || at.y < 0 || at.x >= size.x || at.y >= size.y) {
        return 1.0;
    }
    return textureLoad(ao_map, at, 0).r;
}

fn shade(style: StyleData, direction: vec3<f32>, position: vec3<f32>, occlusion: f32) -> vec4<f32> {
    let normal = normalize(direction);
    let view = view_direction(position);
    let lit_normal = select(normal, -normal, dot(normal, mesh.light.xyz) < 0.0);
    let facing_normal = select(normal, -normal, dot(normal, view) < 0.0);
    let sun = sun_transmittance(
        shadow_coordinate(mesh.light_view, position + lit_normal * mesh.light_reach.z),
        shadow_bias(mesh.light_reach, lit_normal, mesh.light.xyz),
    );
    let strength = clamp(mesh.shadow.x, 0.0, 1.0);
    let visibility = mix(1.0, sun, strength);
    var color = shade_surface(
        style.color.rgb,
        style.surface.x,
        style.surface.y,
        facing_normal,
        view,
        mesh.light.xyz,
        visibility,
        mesh.shading.x,
        mesh.shading.y,
        occlusion,
    );
    if (style.flags.y > 0.5) {
        color = mix(color, vec3<f32>(1.0, 0.86, 0.45), 0.55);
    }
    return vec4<f32>(color, style.color.a);
}
