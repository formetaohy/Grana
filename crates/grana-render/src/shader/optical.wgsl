const OPTICAL_MINIMUM_TAU: f32 = 1.0e-8;

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
    if (opacity <= 0.0 || channel_maximum(channel)) {
        return vec4<f32>(0.0);
    }
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

fn optical_sample(texel: vec3<f32>) -> vec4<f32> {
    let count = u32(frame.march.z);
    let packs = (count + PACK - 1u) / PACK;
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
            let row = channel_optical(channel, values[component]);
            tau += row.a;
            emission += row.rgb;
        }
    }
    return vec4<f32>(emission / max(tau, OPTICAL_MINIMUM_TAU), tau);
}

fn optical_texel(texel: vec3<i32>) -> vec4<f32> {
    let count = u32(frame.march.z);
    let packs = (count + PACK - 1u) / PACK;
    var tau = 0.0;
    var emission = vec3<f32>(0.0);
    for (var pack = 0u; pack < packs; pack += 1u) {
        if (!pack_active(pack, count)) {
            continue;
        }
        let values = textureLoad(atlases[pack], texel, 0);
        for (var component = 0u; component < PACK; component += 1u) {
            let channel = pack * PACK + component;
            if (channel >= count) {
                break;
            }
            let row = channel_optical(channel, values[component]);
            tau += row.a;
            emission += row.rgb;
        }
    }
    return vec4<f32>(emission / max(tau, OPTICAL_MINIMUM_TAU), tau);
}

fn structure_density(texel: vec3<i32>) -> f32 {
    let count = u32(frame.march.z);
    let packs = (count + PACK - 1u) / PACK;
    var total = 0.0;
    for (var pack = 0u; pack < packs; pack += 1u) {
        let values = textureLoad(atlases[pack], texel, 0);
        for (var component = 0u; component < PACK; component += 1u) {
            let channel = pack * PACK + component;
            if (channel >= count) {
                break;
            }
            total += max(values[component], 0.0);
        }
    }
    return total;
}
