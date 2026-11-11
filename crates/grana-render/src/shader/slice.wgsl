fn slice_color(point: vec3<f32>) -> vec4<f32> {
    let uvw = level_local(0u, point);
    let count = u32(frame.march.z);
    let packs = (count + PACK - 1u) / PACK;
    let bands = band_count();
    for (var band = 0u; band < bands; band += 1u) {
        let g = level_frame(band);
        let local = band_uvw(band, uvw);
        let cell = brick_cell(g.grid, g.dims, local);
        let record = level_record(band, brick_index(g.grid, cell));
        if (brick_hollow(record)) {
            return vec4<f32>(0.0);
        }
        if (brick_slot(record) == 0u) {
            continue;
        }
        var radiance = vec3<f32>(0.0);
        var transmittance = 1.0;
        for (var pack = 0u; pack < packs; pack += 1u) {
            if (!pack_active(pack, count)) {
                continue;
            }
            let values = sample_pack(band, pack, local);
            for (var component = 0u; component < PACK; component += 1u) {
                let channel = pack * PACK + component;
                if (channel >= count) {
                    break;
                }
                let opacity = channel_opacity(channel);
                if (opacity <= 0.0 || values[component] < channel_threshold(channel)) {
                    continue;
                }
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
