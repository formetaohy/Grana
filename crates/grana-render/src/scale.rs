const MINIMUM_SIDE: u32 = 1;

pub(crate) fn size(size: [u32; 2], scale: f32) -> [u32; 2] {
    assert!(
        scale.is_finite() && scale > 0.0 && scale <= 1.0,
        "a render scale lies between zero and one"
    );
    [
        ((size[0] as f32 * scale).round() as u32).max(MINIMUM_SIDE),
        ((size[1] as f32 * scale).round() as u32).max(MINIMUM_SIDE),
    ]
}

pub(crate) fn region(region: [u32; 4], scale: f32, size: [u32; 2]) -> [u32; 4] {
    let low_x = ((region[0] as f32) * scale).round() as u32;
    let low_y = ((region[1] as f32) * scale).round() as u32;
    let high_x = ((region[0] as f32 + region[2] as f32) * scale).round() as u32;
    let high_y = ((region[1] as f32 + region[3] as f32) * scale).round() as u32;
    let low_x = low_x.min(size[0] - MINIMUM_SIDE);
    let low_y = low_y.min(size[1] - MINIMUM_SIDE);
    let width = high_x.saturating_sub(low_x).max(MINIMUM_SIDE);
    let height = high_y.saturating_sub(low_y).max(MINIMUM_SIDE);
    [
        low_x,
        low_y,
        width.min(size[0] - low_x),
        height.min(size[1] - low_y),
    ]
}

pub(crate) fn point(point: [u32; 2], scale: f32) -> [u32; 2] {
    [
        ((point[0] as f32) * scale).floor() as u32,
        ((point[1] as f32) * scale).floor() as u32,
    ]
}
