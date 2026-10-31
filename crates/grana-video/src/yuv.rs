const LUMA_RED: f32 = 0.2126;
const LUMA_GREEN: f32 = 0.7152;
const LUMA_BLUE: f32 = 0.0722;
const CHROMA_RED: f32 = 1.574_8;
const CHROMA_BLUE: f32 = 1.855_6;
const BLACK: f32 = 16.0;
const LUMA_SCALE: f32 = 219.0;
const CHROMA_SCALE: f32 = 224.0;
const CHROMA_OFFSET: f32 = 128.0;

pub(crate) struct Planes<'a> {
    pub y: &'a mut [u8],
    pub u: &'a mut [u8],
    pub v: &'a mut [u8],
}

pub(crate) fn from_rgba(rgba: &[u8], size: [u32; 2], planes: Planes<'_>) {
    let width = size[0] as usize;
    let height = size[1] as usize;
    assert_eq!(
        rgba.len(),
        width * height * 4,
        "an rgba frame covers its size"
    );
    assert_eq!(
        planes.y.len(),
        width * height,
        "the luma plane covers its frame"
    );
    assert_eq!(
        planes.u.len(),
        width * height / 4,
        "the chroma plane covers a quarter of its frame"
    );
    assert_eq!(
        planes.v.len(),
        width * height / 4,
        "the chroma plane covers a quarter of its frame"
    );
    for row in 0..height {
        for column in 0..width {
            let index = (row * width + column) * 4;
            let (red, green, blue) = component(rgba, index);
            let value = luma(red, green, blue);
            planes.y[row * width + column] = quantise(BLACK + LUMA_SCALE * value);
        }
    }
    let chroma_width = width / 2;
    for row in 0..height / 2 {
        for column in 0..chroma_width {
            let mut sum = [0.0f32; 3];
            for (row_offset, column_offset) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
                let index = ((2 * row + row_offset) * width + 2 * column + column_offset) * 4;
                let (red, green, blue) = component(rgba, index);
                sum[0] += red;
                sum[1] += green;
                sum[2] += blue;
            }
            let red = sum[0] / 4.0;
            let green = sum[1] / 4.0;
            let blue = sum[2] / 4.0;
            let value = luma(red, green, blue);
            planes.u[row * chroma_width + column] =
                quantise(CHROMA_OFFSET + CHROMA_SCALE * (blue - value) / CHROMA_BLUE);
            planes.v[row * chroma_width + column] =
                quantise(CHROMA_OFFSET + CHROMA_SCALE * (red - value) / CHROMA_RED);
        }
    }
}

fn component(rgba: &[u8], index: usize) -> (f32, f32, f32) {
    (
        f32::from(rgba[index]) / 255.0,
        f32::from(rgba[index + 1]) / 255.0,
        f32::from(rgba[index + 2]) / 255.0,
    )
}

fn luma(red: f32, green: f32, blue: f32) -> f32 {
    red * LUMA_RED + green * LUMA_GREEN + blue * LUMA_BLUE
}

fn quantise(value: f32) -> u8 {
    value.round().clamp(0.0, 255.0) as u8
}
