fn march_projected(
    origin: vec3<f32>,
    begin: f32,
    finish: f32,
    direction: vec3<f32>,
    dither: f32,
) -> March {
    var radiance = vec3<f32>(0.0);
    var transmittance = 1.0;
    var peak = array<f32, CHANNEL_LIMIT>();
    for (var channel = 0u; channel < CHANNEL_LIMIT; channel += 1u) {
        peak[channel] = -1.0;
    }
    let count = u32(frame.march.z);
    let packs = (count + PACK - 1u) / PACK;
    if (finish <= begin) {
        return March(radiance, transmittance);
    }
    var t = begin;
    var samples = 0u;
    var cells = 0u;
    var complete = false;
    while (t < finish && samples < frame.bounds.x && cells < frame.bounds.y && !complete) {
        cells += 1u;
        let found = volume_reachable(origin, band_for(t, dither), t, direction);
        let band = found.band;
        let direction_uvw = level_rate(band, direction);
        let cell_step = local_step(t);
        let exit = boundary_delta(band, found.uvw, direction_uvw);
        let boundary = min(t + max(exit, max(t, 1.0) * 1.0e-6), finish);
        if (!found.usable) {
            let gap = cell_step * ceil(max(boundary - t, 0.0) / cell_step);
            t = min(t + max(gap, cell_step), finish);
            continue;
        }
        let g = level_frame(band);
        let brick = found.cell;
        let record = level_record(band, brick_index(g.grid, brick));
        let slot = brick_display_slot(record) - 1u;
        let voxels_per_world = length(direction_uvw * g.dims);
        var cursor = t;
        while (cursor < boundary && samples < frame.bounds.x && cells < frame.bounds.y && !complete) {
            cells += 1u;
            let stride = local_step(cursor);
            let local = ray_uvw(origin, band, cursor, direction);
            let texel = brick_texel(band, local, brick, slot);
            var tau = 0.0;
            var emission = vec3<f32>(0.0);
            for (var pack = 0u; pack < packs; pack += 1u) {
                if (!pack_active(pack, count)) {
                    continue;
                }
                let values = textureSampleLevel(atlases[pack], atlas_sampler, texel, 0.0);
                for (var component = 0u; component < PACK; component += 1u) {
                    let channel = pack * PACK + component;
                    if (channel >= count) {
                        break;
                    }
                    if (channel_opacity(channel) <= 0.0) {
                        continue;
                    }
                    if (channel_maximum(channel)) {
                        peak[channel] = max(peak[channel], values[component]);
                        continue;
                    }
                    let row = channel_optical(channel, values[component]);
                    tau += row.a;
                    emission += row.rgb;
                }
            }
            let albedo = emission / max(tau, OPTICAL_MINIMUM_TAU);
            let segment = min(stride, finish - cursor);
            let weight = tau * segment * voxels_per_world;
            if (weight > 0.0) {
                let alpha = 1.0 - exp(-weight);
                radiance += transmittance * alpha * structure_lighting(albedo, texel, origin + direction * cursor);
                transmittance *= 1.0 - alpha;
            }
            cursor += segment;
            samples += 1u;
            if (transmittance < MINIMUM_TRANSMITTANCE) {
                complete = true;
            }
        }
        t = max(cursor, boundary);
    }
    for (var channel = 0u; channel < count; channel += 1u) {
        if (!channel_maximum(channel) || peak[channel] < 0.0) {
            continue;
        }
        let opacity = channel_opacity(channel);
        if (opacity <= 0.0) {
            continue;
        }
        let color = channel_tau(channel, peak[channel]).rgb;
        radiance += transmittance * opacity * color;
        transmittance *= 1.0 - opacity;
    }
    return March(radiance, transmittance);
}

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
    let segment = march_projected(
        resolution.origin,
        resolution.start + min(resolution.jitter, max(resolution.exit - resolution.start, 0.0)),
        resolution.exit,
        resolution.direction,
        resolution.dither,
    );
    var radiance = segment.radiance;
    var transmittance = segment.transmittance;
    for (var index = 0u; index < layers.count; index += 1u) {
        let layer = vec4<f32>(
            unpack2x16float(layers.color[index].x),
            unpack2x16float(layers.color[index].y),
        );
        radiance += transmittance * layer.rgb;
        transmittance *= 1.0 - layer.a;
        if (transmittance < MINIMUM_TRANSMITTANCE) {
            break;
        }
    }
    return vec4<f32>(radiance, 1.0 - transmittance);
}
