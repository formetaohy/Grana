@group(1) @binding(0) var volume_radiance: texture_2d<f32>;
@group(1) @binding(1) var volume_surface: texture_2d<f32>;

struct Slab {
    radiance: vec3<f32>,
    opacity: f32,
    depth: f32,
}

fn resolve_slab(clip_position: vec4<f32>) -> Slab {
    let at = vec2<i32>(clip_position.xy);
    let radiance = textureLoad(volume_radiance, at, 0);
    let surface = textureLoad(volume_surface, at, 0);
    return Slab(radiance.rgb, radiance.a, surface.y);
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let slab = resolve_slab(input.clip_position);
    let layers = append_residual(gather_layers(input.clip_position), input.clip_position);
    var boundary = 0u;
    while (boundary < layers.count && layers.depth[boundary] < slab.depth) {
        boundary += 1u;
    }
    if (slab.depth >= 1.0) {
        boundary = 0u;
    }
    var radiance = vec3<f32>(0.0);
    var transmittance = 1.0;
    for (var index = layers.count; index > boundary; index -= 1u) {
        let layer = layer_color(layers.color[index - 1u]);
        radiance = layer.rgb + (1.0 - layer.a) * radiance;
        transmittance *= 1.0 - layer.a;
    }
    radiance = slab.radiance + (1.0 - slab.opacity) * radiance;
    transmittance *= 1.0 - slab.opacity;
    for (var index = boundary; index > 0u; index -= 1u) {
        let layer = layer_color(layers.color[index - 1u]);
        radiance = layer.rgb + (1.0 - layer.a) * radiance;
        transmittance *= 1.0 - layer.a;
    }
    return vec4<f32>(radiance, 1.0 - transmittance);
}
