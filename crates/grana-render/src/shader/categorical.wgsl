fn categorical_presence(tap: BrickTap, local: vec3<f32>, channel: u32) -> f32 {
    let pack = channel / PACK;
    let held = clamp(local, vec3<f32>(0.0), vec3<f32>(BRICK_SIDE));
    let values = tap_fetch(pack, tap_atlas_texel(tap), held);
    return select(0.0, 1.0, !channel_empty(channel, values[channel % PACK]));
}

fn categorical_normal(tap: BrickTap, local: vec3<f32>, channel: u32) -> vec3<f32> {
    let held = clamp(local, vec3<f32>(0.0), vec3<f32>(BRICK_SIDE));
    let x = categorical_presence(tap, held + vec3<f32>(1.0, 0.0, 0.0), channel)
        - categorical_presence(tap, held - vec3<f32>(1.0, 0.0, 0.0), channel);
    let y = categorical_presence(tap, held + vec3<f32>(0.0, 1.0, 0.0), channel)
        - categorical_presence(tap, held - vec3<f32>(0.0, 1.0, 0.0), channel);
    let z = categorical_presence(tap, held + vec3<f32>(0.0, 0.0, 1.0), channel)
        - categorical_presence(tap, held - vec3<f32>(0.0, 0.0, 1.0), channel);
    let gradient = volume_gradient(vec3<f32>(x, y, z));
    if (dot(gradient, gradient) <= 1.0e-20) {
        return frame.forward.xyz;
    }
    return normalize(-gradient);
}

fn categorical_lighting(albedo: vec3<f32>, normal: vec3<f32>, world: vec3<f32>) -> vec3<f32> {
    let cosine = max(dot(normal, frame.light.xyz), 0.0);
    var visibility = 1.0;
    let strength = clamp(frame.march.y, 0.0, 1.0);
    if (strength > 0.0) {
        let lifted = world + normal * frame.light_reach.z;
        let sun = sun_transmittance(
            shadow_coordinate(frame.light_view, lifted),
            shadow_bias(frame.light_reach, normal, frame.light.xyz),
        );
        visibility = mix(1.0, sun, strength);
    }
    let brightness = min(frame.shading.x + frame.shading.y * cosine * visibility, 1.0);
    return albedo * brightness;
}
