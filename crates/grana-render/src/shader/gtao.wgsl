const SLICES: u32 = 8u;
const STEPS: u32 = 5u;

fn pixel_of(world: vec3<f32>) -> vec4<f32> {
    let clip = ao.view_projection * vec4<f32>(world, 1.0);
    if (clip.w <= FLOOR) {
        return vec4<f32>(0.0);
    }
    let ndc = clip.xy / clip.w;
    return vec4<f32>(
        (ndc.x * 0.5 + 0.5) * ao.region.z + ao.region.x,
        (0.5 - ndc.y * 0.5) * ao.region.w + ao.region.y,
        clip.z / clip.w,
        1.0,
    );
}

fn dither(seed: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(seed, vec2<f32>(0.06711056, 0.00583715))));
}

@fragment
fn gtao_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let pixel = vec2<i32>(floor(position.xy)) * AO_SCALE;
    let extent = vec2<i32>(ao.extent.xy);
    let surface = visible_surface(pixel);
    if (!surface.known || length(surface.normal) < NORMAL_REACH) {
        return vec4<f32>(1.0);
    }
    let world = world_of(region_ndc(vec2<f32>(pixel), ao.region), surface.depth);
    let view = normalize(ao.eye.xyz - world);
    let normal = normalize(surface.normal);
    let facing = select(normal, -normal, dot(normal, view) < 0.0);
    let across = cross(facing, view);
    let across_length = length(across);
    let tangent = select(
        vec3<f32>(1.0, 0.0, 0.0),
        across / across_length,
        across_length > FLOOR,
    );
    let bitangent = cross(facing, tangent);
    let focal = max(ao.noise.y, FLOOR);
    let depth_scale = select(1.0, length(world - ao.eye.xyz), ao.noise.z > 0.5);
    let radius = max(ao.extent.z * depth_scale / focal, FLOOR);
    let rotation = dither(vec2<f32>(pixel)) * 6.2831853 + ao.noise.x * 2.39996323;
    var occlusion = 0.0;
    for (var slice = 0u; slice < SLICES; slice += 1u) {
        let angle = rotation + PI * f32(slice) / f32(SLICES);
        let direction = tangent * cos(angle) + bitangent * sin(angle);
        for (var side = 0u; side < 2u; side += 1u) {
            let signum = select(1.0, -1.0, side == 1u);
            var horizon = 0.0;
            for (var step = 1u; step <= STEPS; step += 1u) {
                let reach = radius * f32(step) / f32(STEPS);
                let probe = world + direction * (signum * reach);
                let projected = pixel_of(probe);
                if (projected.w < 0.5) {
                    continue;
                }
                let sample_pixel = vec2<i32>(floor(projected.xy));
                if (sample_pixel.x < 0
                    || sample_pixel.y < 0
                    || sample_pixel.x >= extent.x
                    || sample_pixel.y >= extent.y) {
                    continue;
                }
                let sample_depth = visible_depth(sample_pixel);
                if (sample_depth >= UNREACHED_DEPTH) {
                    continue;
                }
                let occluder = world_of(
                    region_ndc(vec2<f32>(sample_pixel), ao.region),
                    sample_depth,
                );
                let offset = occluder - world;
                let distance = length(offset);
                if (distance < FLOOR || distance > radius * 1.5) {
                    continue;
                }
                let cosine = dot(offset / distance, facing);
                if (cosine <= 0.0) {
                    continue;
                }
                let falloff = 1.0 - clamp(distance / radius, 0.0, 1.0);
                horizon = max(horizon, cosine * falloff);
            }
            occlusion += horizon;
        }
    }
    return vec4<f32>(clamp(1.0 - ao.extent.w * occlusion / f32(SLICES * 2u), 0.0, 1.0));
}
