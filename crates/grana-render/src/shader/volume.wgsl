@fragment
fn fs_main(input: VertexOutput) -> VolumeOutput {
    let ndc = input.ndc + frame.jitter.xy;
    if (slice_active()) {
        let crossing = slice_crossing(camera_ray(ndc).origin, camera_ray(ndc).direction);
        if (crossing.w < 0.5) {
            return VolumeOutput(vec4<f32>(0.0), vec4<f32>(0.0, 0.0, 0.0, 1.0));
        }
        return VolumeOutput(
            slice_color(crossing.xyz),
            vec4<f32>(0.0, 0.0, 0.0, clip_depth(crossing.xyz)),
        );
    }
    let resolution = resolve(input.clip_position, ndc);
    let layers = append_residual(gather_layers(input.clip_position), input.clip_position);
    var radiance = vec3<f32>(0.0);
    var transmittance = 1.0;
    var moment = 0.0;
    var mass = 0.0;
    var hit = vec4<f32>(0.0);
    var hit_normal = vec4<f32>(0.0);
    var cursor = resolution.start + min(resolution.jitter, max(resolution.exit - resolution.start, 0.0));
    for (var index = 0u; index < layers.count; index += 1u) {
        let limit = min(object_distance(resolution.origin, layers.depth[index], ndc), resolution.exit);
        if (limit > cursor) {
            let segment = march_volume(
                resolution.origin,
                cursor,
                limit,
                resolution.direction,
                resolution.dither,
            );
            moment += transmittance * segment.moment;
            mass += transmittance * segment.mass;
            radiance += transmittance * segment.radiance;
            transmittance *= segment.transmittance;
            if (hit.w <= 0.0 && segment.hit.w > 0.0) {
                hit = segment.hit;
                hit_normal = segment.normal;
            }
            cursor = limit;
        }
        let layer = vec4<f32>(
            unpack2x16float(layers.color[index].x),
            unpack2x16float(layers.color[index].y),
        );
        let absorbed = transmittance * layer.a;
        moment += absorbed * layers.depth[index];
        mass += absorbed;
        radiance += transmittance * layer.rgb;
        transmittance *= 1.0 - layer.a;
        if (transmittance < MINIMUM_TRANSMITTANCE) {
            break;
        }
    }
    if (cursor < resolution.exit) {
        let segment = march_volume(
            resolution.origin,
            cursor,
            resolution.exit,
            resolution.direction,
            resolution.dither,
        );
        moment += transmittance * segment.moment;
        mass += transmittance * segment.mass;
        radiance += transmittance * segment.radiance;
        transmittance *= segment.transmittance;
        if (hit.w <= 0.0 && segment.hit.w > 0.0) {
            hit = segment.hit;
            hit_normal = segment.normal;
        }
    }
    var depth = 1.0;
    if (hit.w > 0.0) {
        depth = clip_depth(hit.xyz);
    } else if (mass > 1.0e-4) {
        depth = clip_depth(resolution.origin + resolution.direction * (moment / mass));
    }
    return VolumeOutput(
        vec4<f32>(radiance, 1.0 - transmittance),
        vec4<f32>(hit_normal.xyz, depth),
    );
}
