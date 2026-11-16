@group(1) @binding(0) var volume_radiance: texture_2d<f32>;
@group(1) @binding(1) var volume_surface: texture_2d<f32>;
@group(2) @binding(0) var object_depth: texture_depth_2d;

const DEPTH_TOLERANCE: f32 = 4.0;
const MINIMUM_DEPTH: f32 = 1.0e-9;
const UNREACHED: f32 = 1.0e19;

struct Slab {
    radiance: vec3<f32>,
    opacity: f32,
    depth: f32,
}

fn resolve_slab(clip_position: vec4<f32>) -> Slab {
    let extent = vec2<i32>(textureDimensions(volume_radiance));
    let local = clip_position.xy / frame.volume.xy - vec2<f32>(0.5);
    let base = vec2<i32>(floor(local));
    let remainder = local - vec2<f32>(base);
    let object = textureLoad(object_depth, vec2<i32>(clip_position.xy), 0);
    var colors: array<vec4<f32>, 4>;
    var surfaces: array<vec2<f32>, 4>;
    var reference = UNREACHED;
    for (var index = 0u; index < 4u; index += 1u) {
        let offset = vec2<i32>(i32(index & 1u), i32(index >> 1u));
        let at = clamp(base + offset, vec2<i32>(0), extent - vec2<i32>(1));
        colors[index] = textureLoad(volume_radiance, at, 0);
        surfaces[index] = textureLoad(volume_surface, at, 0).xy;
        if (surfaces[index].y < 1.0) {
            reference = min(reference, surfaces[index].x);
        }
    }
    let guided = reference < UNREACHED;
    let tolerance = max(DEPTH_TOLERANCE * pixel_world(reference), MINIMUM_DEPTH);
    var total = 0.0;
    var radiance = vec3<f32>(0.0);
    var opacity = 0.0;
    var weighted = 0.0;
    var phase = 0.0;
    for (var index = 0u; index < 4u; index += 1u) {
        let corner = vec2<f32>(f32(index & 1u), f32(index >> 1u));
        let bilinear = select(vec2<f32>(1.0) - remainder, remainder, corner > vec2<f32>(0.5));
        var weight = bilinear.x * bilinear.y;
        let surface = surfaces[index];
        if (guided && surface.y < 1.0) {
            weight /= 1.0 + abs(surface.x - reference) / tolerance;
            if (object < 1.0 && surface.y >= object) {
                weight = 0.0;
            }
        }
        total += weight;
        radiance += weight * colors[index].rgb;
        opacity += weight * colors[index].a;
        weighted += weight * colors[index].a * surface.y;
        phase += weight * colors[index].a;
    }
    if (total <= MINIMUM_DEPTH) {
        return Slab(vec3<f32>(0.0), 0.0, 1.0);
    }
    let depth = select(1.0, weighted / phase, phase > MINIMUM_DEPTH);
    return Slab(radiance / total, opacity / total, depth);
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
