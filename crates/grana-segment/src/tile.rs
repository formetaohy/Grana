const PADDING_ALIGNMENT: u32 = 16;
const PADDING_EXTRA: u32 = 16;
const PADDING_HALF: u32 = 8;
const TAPER_MINIMUM: usize = 224;
const TAPER_SIGMA: f32 = 7.5;
const TAPER_EDGE: f32 = 20.0;
const OVERLAP_MINIMUM: f64 = 0.05;
const OVERLAP_MAXIMUM: f64 = 0.5;

pub(crate) fn padding(length: u32, side: u32) -> (u32, u32) {
    assert!(side > 0, "the model side is positive");
    let align = if length >= side {
        (PADDING_ALIGNMENT - length % PADDING_ALIGNMENT) % PADDING_ALIGNMENT
    } else {
        (side - length).saturating_sub(PADDING_EXTRA)
    };
    let lead = PADDING_HALF + align / 2;
    let trail = PADDING_HALF + align - align / 2;
    (lead, trail)
}

pub(crate) fn starts(length: u32, side: u32, overlap: u32) -> Vec<u32> {
    assert!(side > 0, "the model side is positive");
    if length <= side {
        return vec![0];
    }
    let fraction =
        (f64::from(overlap.min(side)) / f64::from(side)).clamp(OVERLAP_MINIMUM, OVERLAP_MAXIMUM);
    let count = ((1.0 + 2.0 * fraction) * f64::from(length) / f64::from(side)).ceil() as u32;
    let count = count.max(2);
    let span = f64::from(length - side);
    (0..count)
        .map(|index| (span * f64::from(index) / f64::from(count - 1)) as u32)
        .collect()
}

pub(crate) fn taper(side: usize, output: &mut [f32]) {
    assert_eq!(output.len(), side * side, "the taper covers the tile");
    let span = side.max(TAPER_MINIMUM);
    let center = (span as f32 - 1.0) * 0.5;
    let edge = span as f32 * 0.5 - TAPER_EDGE;
    let mut line = vec![0f32; span];
    for (index, value) in line.iter_mut().enumerate() {
        let offset = (index as f32 - center).abs();
        *value = 1.0 / (1.0 + ((offset - edge) / TAPER_SIGMA).exp());
    }
    let start = span / 2 - side / 2;
    for row in 0..side {
        for column in 0..side {
            output[row * side + column] = line[start + row] * line[start + column];
        }
    }
}
