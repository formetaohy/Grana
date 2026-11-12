fn march_projected(
    origin: vec3<f32>,
    begin: f32,
    finish: f32,
    direction: vec3<f32>,
    dither: f32,
) -> March {
    var radiance = vec3<f32>(0.0);
    var transmittance = 1.0;
    var moment = 0.0;
    var mass = 0.0;
    var peak = array<f32, CHANNEL_LIMIT>();
    for (var channel = 0u; channel < CHANNEL_LIMIT; channel += 1u) {
        peak[channel] = -1.0;
    }
    let count = u32(frame.march.z);
    let packs = (count + PACK - 1u) / PACK;
    if (finish <= begin) {
        return March(radiance, transmittance, moment, mass, vec4<f32>(0.0), vec4<f32>(0.0));
    }
    let extent = vec3<f32>(frame.atlas.xyz) * PADDED_SIDE;
    var t = begin;
    var samples = 0u;
    var complete = false;
    while (t < finish && samples < SAMPLE_LIMIT && !complete) {
        let found = volume_reachable(origin, band_for(t, dither), t, direction);
        if (!found.usable) {
            let skip = ray_uvw(origin, found.band, t, direction);
            let rate = level_rate(found.band, direction);
            let edge = boundary_delta(found.band, skip, rate, local_step(t) * 0.25);
            t = min(t + edge, finish);
            continue;
        }
        let band = found.band;
        let g = level_frame(band);
        let direction_uvw = level_rate(band, direction);
        let brick = found.cell;
        let boundary = min(
            t + boundary_delta(band, found.uvw, direction_uvw, local_step(t) * 0.25),
            finish,
        );
        let record = level_record(band, brick_index(g.grid, brick));
        let slot = brick_display_slot(record) - 1u;
        let voxels_per_world = length(direction_uvw * g.dims);
        let texel_rate = direction_uvw * g.dims / extent;
        var texel = (slot_origin(frame.atlas.xyz, slot)
            + (clamp(found.uvw, vec3<f32>(0.0), vec3<f32>(1.0)) * g.dims - brick_origin(brick))
            + vec3<f32>(1.0)) / extent;
        var cursor = t;
        while (cursor < boundary && samples < SAMPLE_LIMIT && !complete) {
            let step = local_step(cursor);
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
            let next = min(cursor + step, boundary);
            let segment = next - cursor;
            let weight = tau * segment * voxels_per_world;
            if (weight > 0.0) {
                let alpha = 1.0 - exp(-weight);
                let absorbed = transmittance * alpha;
                radiance += absorbed * volume_lighting(albedo, texel, origin + direction * cursor);
                moment += absorbed * cursor;
                mass += absorbed;
                transmittance *= 1.0 - alpha;
            }
            texel += texel_rate * segment;
            cursor = next;
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
    return March(radiance, transmittance, moment, mass, vec4<f32>(0.0), vec4<f32>(0.0));
}

@fragment
fn fs_main(input: VertexOutput) -> VolumeOutput {
    let ndc = input.ndc + frame.jitter.xy;
    if (slice_active()) {
        let ray = camera_ray(ndc);
        let crossing = slice_crossing(ray.origin, ray.direction);
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
    let segment = march_projected(
        resolution.origin,
        resolution.start + min(resolution.jitter, max(resolution.exit - resolution.start, 0.0)),
        resolution.exit,
        resolution.direction,
        resolution.dither,
    );
    var radiance = segment.radiance;
    var transmittance = segment.transmittance;
    var moment = segment.moment;
    var mass = segment.mass;
    for (var index = 0u; index < layers.count; index += 1u) {
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
    var depth = 1.0;
    if (mass > 1.0e-4) {
        depth = clip_depth(resolution.origin + resolution.direction * (moment / mass));
    }
    return VolumeOutput(
        vec4<f32>(radiance, 1.0 - transmittance),
        vec4<f32>(0.0, 0.0, 0.0, depth),
    );
}
