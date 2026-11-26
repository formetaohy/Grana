const OPTICAL_MINIMUM_TAU: f32 = 1.0e-8;
const SURFACE_EXTINCTION: f32 = 32.0;

struct Optical {
    emission: vec3<f32>,
    extinction: f32,
    alpha: f32,
    surface: u32,
    peak: bool,
}

struct ChannelSample {
    emission: vec3<f32>,
    extinction: f32,
    alpha: f32,
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

fn categorical_sample(channel: u32, value: f32) -> ChannelSample {
    if (value < channel_threshold(channel)) {
        return ChannelSample(vec3<f32>(0.0), 0.0, 0.0);
    }
    let weight = channel_opacity(channel);
    return ChannelSample(channel_tau(channel, value).rgb * weight, weight, weight);
}

fn channel_optical(channel: u32, value: f32) -> ChannelSample {
    if (channel_categorical(channel) && !channel_maximum(channel)) {
        return categorical_sample(channel, value);
    }
    let opacity = channel_opacity(channel);
    if (channel_isosurface(channel)) {
        let density = iso_density(channel, value) * opacity;
        return ChannelSample(channel_tau(channel, value).rgb * density, density, 0.0);
    }
    if (value < channel_threshold(channel)) {
        return ChannelSample(vec3<f32>(0.0), 0.0, 0.0);
    }
    let extinction = channel_tau(channel, value);
    let weight = extinction.a * opacity;
    return ChannelSample(extinction.rgb * weight, weight, 0.0);
}

fn channel_extinction(channel: u32, value: f32) -> f32 {
    if (channel_maximum(channel)) {
        return 0.0;
    }
    let opacity = channel_opacity(channel);
    if (channel_categorical(channel)) {
        if (value < channel_threshold(channel)) {
            return 0.0;
        }
        return SURFACE_EXTINCTION * opacity;
    }
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
    var alpha = 0.0;
    var surface = 0u;
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
                if (value >= channel_threshold(channel) && value > (*peaks)[channel]) {
                    (*peaks)[channel] = value;
                    grown = true;
                }
                continue;
            }
            if (!channel_active(channel)) {
                continue;
            }
            let row = channel_optical(channel, values[component]);
            extinction += row.extinction;
            emission += row.emission;
            if (row.alpha > alpha) {
                alpha = row.alpha;
                surface = channel + 1u;
            }
        }
    }
    return Optical(emission, extinction, alpha, surface, grown);
}
