const OPTICAL_MINIMUM_TAU: f32 = 1.0e-8;

struct Optical {
    emission: vec3<f32>,
    extinction: f32,
    peak: bool,
}

struct ChannelSample {
    emission: vec3<f32>,
    extinction: f32,
}

fn iso_density(channel: u32, value: f32) -> f32 {
    let inverse = channels[channel].inverse;
    if (inverse <= 0.0) {
        return 0.0;
    }
    let softness = ISO_SOFTNESS / inverse;
    let threshold = channel_threshold(channel);
    return ISO_EXTINCTION * smoothstep(threshold - softness, threshold + softness, value);
}

fn channel_optical(channel: u32, value: f32) -> ChannelSample {
    let opacity = channel_opacity(channel);
    if (channel_isosurface(channel)) {
        let density = iso_density(channel, value) * opacity;
        return ChannelSample(channel_tau(channel, value).rgb * density, density);
    }
    if (channel_empty(channel, value)) {
        return ChannelSample(vec3<f32>(0.0), 0.0);
    }
    let extinction = channel_tau(channel, value);
    let weight = extinction.a * opacity;
    return ChannelSample(extinction.rgb * weight, weight);
}

fn channel_extinction(channel: u32, value: f32) -> f32 {
    if (channel_maximum(channel) || channel_surface(channel)) {
        return 0.0;
    }
    let opacity = channel_opacity(channel);
    if (channel_isosurface(channel)) {
        return iso_density(channel, value) * opacity;
    }
    return channel_tau(channel, value).a * opacity;
}

fn optical_sample(
    tap: BrickTap,
    local: vec3<f32>,
    peaks: ptr<function, array<f32, CHANNEL_LIMIT>>,
) -> Optical {
    var extinction = 0.0;
    var emission = vec3<f32>(0.0);
    var grown = false;
    let texel = tap_atlas_texel(tap);
    for (var pack = 0u; pack < PACK_LIMIT; pack += 1u) {
        if (!pack_active(pack)) {
            continue;
        }
        let values = tap_fetch(pack, texel, local);
        for (var component = 0u; component < PACK; component += 1u) {
            let channel = pack * PACK + component;
            if (channel_peak(channel)) {
                let value = values[component];
                if (!channel_empty(channel, value) && value > (*peaks)[channel]) {
                    (*peaks)[channel] = value;
                    grown = true;
                }
                continue;
            }
            if (!channel_active(channel) || channel_surface(channel)) {
                continue;
            }
            let row = channel_optical(channel, values[component]);
            extinction += row.extinction;
            emission += row.emission;
        }
    }
    return Optical(emission, extinction, grown);
}
