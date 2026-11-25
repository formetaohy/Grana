fn slice_color(point: vec3<f32>) -> vec4<f32> {
    let uvw = level_local(0u, point);
    let bands = band_count();
    for (var band = 0u; band < bands; band += 1u) {
        let g = level_frame(band);
        let voxel = band_uvw(band, uvw) * g.dims;
        let cell = brick_of(g.grid, g.dims, voxel);
        let record = level_record(band, brick_index(g.grid, cell));
        if (brick_hollow(record)) {
            return vec4<f32>(0.0);
        }
        if (brick_display_slot(record) == 0u) {
            continue;
        }
        var radiance = vec3<f32>(0.0);
        var transmittance = 1.0;
        for (var pack = 0u; pack < PACK_LIMIT; pack += 1u) {
            if (!pack_active(pack)) {
                continue;
            }
            let values = tap_sample(band, pack, voxel);
            for (var component = 0u; component < PACK; component += 1u) {
                let channel = pack * PACK + component;
                if (!channel_active(channel)) {
                    continue;
                }
                if (values[component] < channel_threshold(channel)) {
                    continue;
                }
                let opacity = channel_opacity(channel);
                let extinction = channel_tau(channel, values[component]);
                var alpha = opacity;
                if (!channel_isosurface(channel) && !channel_maximum(channel)) {
                    alpha = 1.0 - exp(-extinction.a * opacity);
                }
                alpha = clamp(alpha, 0.0, 0.999);
                radiance += transmittance * alpha * extinction.rgb;
                transmittance *= 1.0 - alpha;
            }
        }
        return vec4<f32>(radiance, 1.0 - transmittance);
    }
    return vec4<f32>(0.0);
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let ray = camera_ray(input.ndc - frame.jitter.xy);
    let crossing = slice_crossing(ray.origin, ray.direction);
    if (crossing.w < 0.5) {
        return vec4<f32>(0.0);
    }
    return slice_color(crossing.xyz);
}
