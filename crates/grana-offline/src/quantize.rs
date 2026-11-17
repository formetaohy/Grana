use grana_render::Pixels;

const BAYER: [[u8; 8]; 8] = [
    [0, 32, 8, 40, 2, 34, 10, 42],
    [48, 16, 56, 24, 50, 18, 58, 26],
    [12, 44, 4, 36, 14, 46, 6, 38],
    [60, 28, 52, 20, 62, 30, 54, 22],
    [3, 35, 11, 43, 1, 33, 9, 41],
    [51, 19, 59, 27, 49, 17, 57, 25],
    [15, 47, 7, 39, 13, 45, 5, 37],
    [63, 31, 55, 23, 61, 29, 53, 21],
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Depth {
    Eight,
    Sixteen,
}

pub(crate) struct Quantizer {
    size: [u32; 2],
    samples: usize,
    depth: Depth,
    transparent: bool,
    grained: bool,
    pixels: Vec<u16>,
}

impl Quantizer {
    pub(crate) fn new(
        size: [u32; 2],
        samples: usize,
        depth: Depth,
        transparent: bool,
        grained: bool,
    ) -> Self {
        assert!(
            samples == 3 || samples == 4,
            "a quantised picture carries three or four samples per pixel"
        );
        assert!(
            depth == Depth::Sixteen || samples == 4,
            "an eight bit quantised picture carries an alpha sample"
        );
        Self {
            size,
            samples,
            depth,
            transparent,
            grained,
            pixels: Vec::new(),
        }
    }

    pub(crate) fn samples(&self) -> usize {
        self.samples
    }

    pub(crate) fn stride(&self) -> usize {
        self.size[0] as usize * self.samples * self.step()
    }

    pub(crate) fn row(&mut self, line: u32, source: &[u8], out: &mut [u8]) {
        let source = source.as_chunks::<16>().0;
        assert_eq!(
            source.len(),
            self.size[0] as usize,
            "a quantised row covers one line of the picture"
        );
        assert_eq!(
            out.len(),
            self.stride(),
            "a quantised row holds every sample of its line"
        );
        self.pixels.clear();
        self.pixels.extend(source.iter().flat_map(|pixel| {
            std::array::from_fn::<u16, 4, _>(|sample| {
                let bytes = &pixel[sample * 4..sample * 4 + 4];
                let value =
                    f32::from_le_bytes(bytes.try_into().expect("a sample spans four bytes"));
                (value.clamp(0.0, 1.0) * 65535.0).round() as u16
            })
        }));
        match self.depth {
            Depth::Sixteen => {
                let mut slots = out.as_chunks_mut::<2>().0.iter_mut();
                for pixel in self.pixels.as_chunks::<4>().0 {
                    let alpha = if self.transparent { pixel[3] } else { u16::MAX };
                    let wanted = [pixel[0], pixel[1], pixel[2], alpha];
                    for value in wanted.into_iter().take(self.samples) {
                        slots
                            .next()
                            .expect("a quantised row holds every sample of its line")
                            .copy_from_slice(&value.to_be_bytes());
                    }
                }
            }
            Depth::Eight => {
                let threshold = if self.grained {
                    BAYER[line as usize % 8].map(|value| f32::from(value) / 64.0 - 0.5)
                } else {
                    [0.0; 8]
                };
                for (x, (slot, pixel)) in out
                    .as_chunks_mut::<4>()
                    .0
                    .iter_mut()
                    .zip(self.pixels.as_chunks::<4>().0)
                    .enumerate()
                {
                    let grain = threshold[x % 8];
                    let [red, green, blue, alpha] = *pixel;
                    slot[0] = scaled(red, grain);
                    slot[1] = scaled(green, grain);
                    slot[2] = scaled(blue, grain);
                    slot[3] = if self.transparent {
                        (u32::from(alpha) * 255 / 65535) as u8
                    } else {
                        255
                    };
                }
            }
        }
    }

    fn step(&self) -> usize {
        match self.depth {
            Depth::Eight => 1,
            Depth::Sixteen => 2,
        }
    }
}

fn scaled(sample: u16, grain: f32) -> u8 {
    let value = f32::from(sample) / 65535.0 * 255.0 + grain;
    value.round().clamp(0.0, 255.0) as u8
}

pub(crate) fn rgba8(picture: Pixels<'_>, transparent: bool) -> Vec<u8> {
    let size = picture.size();
    let mut quantizer = Quantizer::new(size, 4, Depth::Eight, transparent, true);
    let stride = quantizer.stride();
    let mut pixels = vec![0u8; stride * size[1] as usize];
    for (line, row) in pixels.chunks_mut(stride).enumerate() {
        quantizer.row(line as u32, picture.row(line as u32), row);
    }
    pixels
}
