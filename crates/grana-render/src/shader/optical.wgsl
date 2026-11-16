const OPTICAL_MINIMUM_TAU: f32 = 1.0e-8;

struct Optical {
    emission: vec3<f32>,
    extinction: f32,
    peak: bool,
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

fn channel_optical(channel: u32, value: f32) -> vec4<f32> {
    let opacity = channel_opacity(channel);
    if (channel_isosurface(channel)) {
        let density = iso_density(channel, value) * opacity;
        return vec4<f32>(channel_tau(channel, value).rgb * density, density);
    }
    if (value < channel_threshold(channel)) {
        return vec4<f32>(0.0);
    }
    let extinction = channel_tau(channel, value);
    let weight = extinction.a * opacity;
    return vec4<f32>(extinction.rgb * weight, weight);
}

fn channel_extinction(channel: u32, value: f32) -> f32 {
    if (channel_maximum(channel)) {
        return 0.0;
    }
    let opacity = channel_opacity(channel);
    if (channel_isosurface(channel)) {
        return iso_density(channel, value) * opacity;
    }
    return channel_tau(channel, value).a * opacity;
}

fn optical_sample(
    texel: vec3<f32>,
    peaks: ptr<function, array<f32, CHANNEL_LIMIT>>,
) -> Optical {
    var tau = 0.0;
    var emission = vec3<f32>(0.0);
    var grown = false;
    for (var pack = 0u; pack < PACK_LIMIT; pack += 1u) {
        if (!pack_active(pack)) {
            continue;
        }
        let values = textureSampleLevel(atlases[pack], atlas_sampler, texel, 0.0);
        for (var component = 0u; component < PACK; component += 1u) {
            let channel = pack * PACK + component;
            if (channel_peak(channel)) {
                let value = values[component];
                if (value > (*peaks)[channel]) {
                    (*peaks)[channel] = value;
                    grown = true;
                }
                continue;
            }
            if (!channel_active(channel)) {
                continue;
            }
            let row = channel_optical(channel, values[component]);
            tau += row.a;
            emission += row.rgb;
        }
    }
    return Optical(emission, tau, grown);
}
