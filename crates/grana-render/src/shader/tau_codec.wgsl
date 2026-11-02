fn encoded_tau(tau: f32) -> f32 {
    return tau / (1.0 + tau);
}

fn decoded_tau(encoded: f32) -> f32 {
    return encoded / max(1.0 - encoded, 1.0e-4);
}
