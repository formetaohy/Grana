@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let ndc = input.ndc + frame.jitter.xy;
    if (slice_active()) {
        let ray = camera_ray(ndc);
        let crossing = slice_crossing(ray.origin, ray.direction);
        if (crossing.w < 0.5) {
            return vec4<f32>(0.0);
        }
        return slice_color(crossing.xyz);
    }
    let resolution = resolve(input.clip_position, ndc);
    let layers = append_residual(gather_layers(input.clip_position), input.clip_position);
    var cursor = resolution.start + min(resolution.jitter, max(resolution.exit - resolution.start, 0.0));
    var journey = journey_begin(
        resolution.origin,
        cursor,
        resolution.exit,
        resolution.direction,
        resolution.dither,
    );
    for (var index = 0u; index < layers.count; index += 1u) {
        let limit = min(object_distance(resolution.origin, layers.depth[index], ndc), resolution.exit);
        if (limit > cursor) {
            journey = journey_march(
                journey,
                resolution.origin,
                cursor,
                limit,
                resolution.exit,
                resolution.direction,
                resolution.dither,
            );
            cursor = limit;
        }
        let layer = vec4<f32>(
            unpack2x16float(layers.color[index].x),
            unpack2x16float(layers.color[index].y),
        );
        journey.march = march_layer(journey.march, layer);
        if (journey.march.transmittance < MINIMUM_TRANSMITTANCE) {
            break;
        }
    }
    if (cursor < resolution.exit) {
        journey = journey_march(
            journey,
            resolution.origin,
            cursor,
            resolution.exit,
            resolution.exit,
            resolution.direction,
            resolution.dither,
        );
    }
    return vec4<f32>(journey.march.radiance, 1.0 - journey.march.transmittance);
}
