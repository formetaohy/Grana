fn srgb_encode(linear: vec3<f32>) -> vec3<f32> {
    let clamped = clamp(linear, vec3<f32>(0.0), vec3<f32>(1.0));
    let low = clamped * 12.92;
    let high = 1.055 * pow(clamped, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(high, low, clamped <= vec3<f32>(0.0031308));
}

fn srgb_decode(encoded: vec3<f32>) -> vec3<f32> {
    let clamped = clamp(encoded, vec3<f32>(0.0), vec3<f32>(1.0));
    let low = clamped / 12.92;
    let high = pow((clamped + 0.055) / 1.055, vec3<f32>(2.4));
    return select(high, low, clamped <= vec3<f32>(0.04045));
}

fn aces(radiance: vec3<f32>) -> vec3<f32> {
    let toe = 0.03;
    let shoulder = 2.43;
    let offset = 0.59;
    let bias = 0.14;
    let slope = 2.51;
    return clamp(
        (radiance * (slope * radiance + toe)) / (radiance * (shoulder * radiance + offset) + bias),
        vec3<f32>(0.0),
        vec3<f32>(1.0),
    );
}

fn background(ndc: vec2<f32>) -> vec3<f32> {
    let height = clamp(ndc.y * 0.5 + 0.5, 0.0, 1.0);
    return mix(vec3<f32>(0.012, 0.014, 0.020), vec3<f32>(0.040, 0.048, 0.066), height);
}

fn dither(position: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(position, vec2<f32>(0.06711056, 0.00583715))));
}

fn interleaved_gradient(position: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(position, vec2<f32>(0.06711056, 0.00583715))));
}
