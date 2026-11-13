use rayon::prelude::*;

const REACH: f32 = 3.0;

const KERNEL_FLOOR: f32 = 1.0e-3;

#[derive(Clone, Debug)]
pub(super) struct Grid {
    size: [u32; 2],
    pixel: f32,
    origin: [f32; 2],
    values: Vec<f32>,
    chroma: Option<[Vec<f32>; 3]>,
}

impl Grid {
    pub(super) fn empty(size: [u32; 2], pixel: f32, origin: [f32; 2]) -> Self {
        Self::new(size, pixel, origin, false)
    }

    pub(super) fn tinted(size: [u32; 2], pixel: f32, origin: [f32; 2]) -> Self {
        Self::new(size, pixel, origin, true)
    }

    fn new(size: [u32; 2], pixel: f32, origin: [f32; 2], tinted: bool) -> Self {
        assert!(
            size[0] > 0 && size[1] > 0,
            "a localization grid covers at least one pixel"
        );
        assert!(
            pixel.is_finite() && pixel > 0.0,
            "a localization grid rasterizes at a positive pixel size"
        );
        assert!(
            origin.iter().all(|value| value.is_finite()),
            "a localization grid starts at a finite origin"
        );
        let cells = size[0] as usize * size[1] as usize;
        Self {
            size,
            pixel,
            origin,
            values: vec![0.0; cells],
            chroma: tinted.then(|| std::array::from_fn(|_| vec![0.0; cells])),
        }
    }

    pub(super) fn size(&self) -> [u32; 2] {
        self.size
    }

    pub(super) fn pixel(&self) -> f32 {
        self.pixel
    }

    pub(super) fn values(&self) -> &[f32] {
        &self.values
    }

    pub(super) fn chroma(&self, index: usize) -> Option<[f32; 3]> {
        self.chroma
            .as_ref()
            .map(|held| std::array::from_fn(|axis| held[axis][index]))
    }

    pub(super) fn index(&self, point: [f32; 2]) -> Option<usize> {
        let column = ((point[0] - self.origin[0]) / self.pixel).floor();
        let row = ((point[1] - self.origin[1]) / self.pixel).floor();
        if !(column >= 0.0 && row >= 0.0) {
            return None;
        }
        let column = column as u32;
        let row = row as u32;
        (column < self.size[0] && row < self.size[1])
            .then_some(column as usize + self.size[0] as usize * row as usize)
    }

    pub(super) fn add(&mut self, point: [f32; 2], weight: f32, tint: Option<[f32; 3]>) {
        assert_eq!(
            self.chroma.is_some(),
            tint.is_some(),
            "a localization grid tints every sample it holds or none of them"
        );
        let Some(index) = self.index(point) else {
            return;
        };
        self.values[index] += weight;
        if let (Some(chroma), Some(tint)) = (self.chroma.as_mut(), tint) {
            for axis in 0..3 {
                chroma[axis][index] += tint[axis] * weight;
            }
        }
    }

    pub(super) fn blur(&mut self, sigma: f32) {
        if !(sigma.is_finite() && sigma > 0.0) {
            return;
        }
        let sigma = (sigma / self.pixel).max(KERNEL_FLOOR);
        let radius = (REACH * sigma).ceil().max(1.0) as usize;
        let kernel = kernel(sigma, radius);
        let width = self.size[0] as usize;
        let height = self.size[1] as usize;
        let mut scratch = vec![0.0f32; width * height];
        convolve(
            &mut self.values,
            &mut scratch,
            width,
            height,
            &kernel,
            radius,
        );
        if let Some(chroma) = self.chroma.as_mut() {
            for plane in chroma.iter_mut() {
                convolve(plane, &mut scratch, width, height, &kernel, radius);
            }
        }
    }

    pub(super) fn maximum(&self) -> f32 {
        self.values.iter().copied().fold(0.0, f32::max)
    }
}

fn kernel(sigma: f32, radius: usize) -> Vec<f32> {
    let mut kernel = Vec::with_capacity(radius * 2 + 1);
    let mut total = 0.0f32;
    for offset in -(radius as i64)..=radius as i64 {
        let distance = offset as f32;
        let weight = (-0.5 * distance * distance / (sigma * sigma)).exp();
        kernel.push(weight);
        total += weight;
    }
    for weight in &mut kernel {
        *weight /= total;
    }
    kernel
}

fn convolve(
    field: &mut [f32],
    scratch: &mut [f32],
    width: usize,
    height: usize,
    kernel: &[f32],
    radius: usize,
) {
    scratch
        .par_chunks_mut(width)
        .enumerate()
        .for_each(|(row, line)| {
            let source = &field[row * width..(row + 1) * width];
            line_convolve(source, kernel, line);
        });
    let held = &*scratch;
    field
        .par_chunks_mut(width)
        .enumerate()
        .for_each(|(row, line)| {
            line.fill(0.0);
            for (offset, weight) in kernel.iter().enumerate() {
                let sample = (row as i64 + offset as i64 - radius as i64)
                    .clamp(0, height as i64 - 1) as usize;
                for (value, held) in line.iter_mut().zip(&held[sample * width..]) {
                    *value += held * weight;
                }
            }
        });
}

fn line_convolve(source: &[f32], kernel: &[f32], target: &mut [f32]) {
    let length = source.len();
    let radius = (kernel.len() - 1) / 2;
    for (index, value) in target.iter_mut().enumerate() {
        let mut sum = 0.0f32;
        for (offset, weight) in kernel.iter().enumerate() {
            let sample = index as i64 + offset as i64 - radius as i64;
            let sample = sample.clamp(0, length as i64 - 1) as usize;
            sum += source[sample] * weight;
        }
        *value = sum;
    }
}
