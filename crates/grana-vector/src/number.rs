pub(crate) fn text(value: f32) -> String {
    assert!(value.is_finite(), "a vector coordinate is finite");
    let rounded = (value * 10_000.0).round() / 10_000.0;
    let mut text = format!("{rounded:.4}");
    if text.contains('.') {
        while text.ends_with('0') {
            text.pop();
        }
        if text.ends_with('.') {
            text.pop();
        }
    }
    if text == "-0" { "0".to_string() } else { text }
}

pub(crate) fn byte(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}
