const BITS_PER_PIXEL: u64 = 4;
const MINIMUM: u64 = 4_000_000;
const MAXIMUM: u64 = 80_000_000;
const OVERSHOOT_NUMERATOR: u64 = 5;
const OVERSHOOT_DENOMINATOR: u64 = 4;

pub(crate) fn bits_per_second(size: [u32; 2], fps: u32) -> u32 {
    let pixels = u64::from(size[0]) * u64::from(size[1]);
    (pixels * u64::from(fps) / BITS_PER_PIXEL).clamp(MINIMUM, MAXIMUM) as u32
}

pub(crate) fn frame_bytes(size: [u32; 2], fps: u32) -> u64 {
    let per_second = u64::from(bits_per_second(size, fps.max(1)));
    let frames = u64::from(fps.max(1));
    per_second.div_ceil(8 * frames) * OVERSHOOT_NUMERATOR / OVERSHOOT_DENOMINATOR
}
