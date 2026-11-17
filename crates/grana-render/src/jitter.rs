use glam::Vec2;

const RADICAL: [u32; 2] = [2, 3];
const REACH: f32 = 0.5;

pub(crate) fn offset(index: u32, size: [u32; 2]) -> Vec2 {
    assert!(
        size[0] > 0 && size[1] > 0,
        "a jittered view covers at least one pixel"
    );
    Vec2::new(
        (halton(index, RADICAL[0]) - 0.5) * 2.0 * REACH / size[0] as f32,
        (halton(index, RADICAL[1]) - 0.5) * 2.0 * REACH / size[1] as f32,
    )
}

fn halton(index: u32, base: u32) -> f32 {
    let mut fraction = 1.0;
    let mut value = 0.0;
    let mut remaining = index;
    while remaining > 0 {
        fraction /= base as f32;
        value += fraction * (remaining % base) as f32;
        remaining /= base;
    }
    value
}
