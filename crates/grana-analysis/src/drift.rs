use std::f32::consts::TAU;
use std::ops::Range;

use grana_volume::{Extent, Pyramid, VoxelData};

const WINDOW: u32 = 64;

const MINIMUM_WINDOW: u32 = 32;
const PEAK_LIMIT: f32 = 0.5;
const TAPER: f32 = 0.5;
const EPSILON: f32 = 1.0e-12;

#[derive(Clone, Copy, Default)]
struct Complex {
    re: f32,
    im: f32,
}

impl Complex {
    fn multiply(self, other: Self) -> Self {
        Self {
            re: self.re * other.re - self.im * other.im,
            im: self.re * other.im + self.im * other.re,
        }
    }

    fn conjugate(self) -> Self {
        Self {
            re: self.re,
            im: -self.im,
        }
    }
}

struct Cube {
    dims: [usize; 3],
    values: Vec<Complex>,
}

impl Cube {
    fn new(dims: [usize; 3]) -> Self {
        Self {
            dims,
            values: vec![Complex::default(); dims[0] * dims[1] * dims[2]],
        }
    }

    fn index(&self, at: [usize; 3]) -> usize {
        at[0] + self.dims[0] * (at[1] + self.dims[1] * at[2])
    }

    fn transform(&mut self, inverse: bool) {
        let mut line = Vec::new();
        for axis in 0..3 {
            let length = self.dims[axis];
            let table = twiddles(length);
            let inner = match axis {
                0 => 1,
                1 => self.dims[0],
                _ => self.dims[0] * self.dims[1],
            };
            let outer = self.values.len() / (length * inner);
            line.resize(length, Complex::default());
            for band in 0..outer {
                for lane in 0..inner {
                    let base = band * length * inner + lane;
                    for (slot, value) in line.iter_mut().enumerate() {
                        *value = self.values[base + slot * inner];
                    }
                    transform_line(&mut line, &table, inverse);
                    for (slot, value) in line.iter().enumerate() {
                        self.values[base + slot * inner] = *value;
                    }
                }
            }
        }
    }
}

fn twiddles(length: usize) -> Vec<Complex> {
    (0..length / 2)
        .map(|index| {
            let angle = -TAU * index as f32 / length as f32;
            Complex {
                re: angle.cos(),
                im: angle.sin(),
            }
        })
        .collect()
}

fn transform_line(line: &mut [Complex], table: &[Complex], inverse: bool) {
    let length = line.len();
    if length < 2 {
        return;
    }
    let bits = length.trailing_zeros();
    for index in 0..length {
        let reversed = index.reverse_bits() >> (usize::BITS - bits);
        if index < reversed {
            line.swap(index, reversed);
        }
    }
    let mut span = 2;
    while span <= length {
        let half = span / 2;
        let step = length / span;
        for start in (0..length).step_by(span) {
            for offset in 0..half {
                let table = table[offset * step];
                let twiddle = if inverse { table.conjugate() } else { table };
                let first = line[start + offset];
                let second = line[start + offset + half].multiply(twiddle);
                line[start + offset] = Complex {
                    re: first.re + second.re,
                    im: first.im + second.im,
                };
                line[start + offset + half] = Complex {
                    re: first.re - second.re,
                    im: first.im - second.im,
                };
            }
        }
        span <<= 1;
    }
    if inverse {
        let scale = 1.0 / length as f32;
        for value in line.iter_mut() {
            value.re *= scale;
            value.im *= scale;
        }
    }
}

pub fn working_level(pyramid: &Pyramid) -> Option<usize> {
    let level = pyramid.stored().checked_sub(1)?;
    (pyramid.level(level).extent().max_dimension() >= MINIMUM_WINDOW).then_some(level)
}

pub fn spans(pyramid: &Pyramid, level: usize) -> [Range<u32>; 3] {
    centered(pyramid.level(level).extent())
}

fn centered(extent: Extent) -> [Range<u32>; 3] {
    let dims = [extent.x(), extent.y(), extent.z()];
    [0, 1, 2].map(|axis| {
        let span = dims[axis].min(WINDOW);
        let start = (dims[axis] - span) / 2;
        start..start + span
    })
}

pub fn window(data: &VoxelData, ranges: &[Range<u32>; 3]) -> Vec<f32> {
    let mut values = vec![
        0.0f32;
        ranges
            .iter()
            .map(|range| (range.end - range.start) as usize)
            .product()
    ];
    data.write_values(&mut values);
    values
}

pub fn shift(reference: &[f32], moving: &[f32], dims: [usize; 3]) -> [i32; 3] {
    assert_eq!(
        reference.len(),
        dims[0] * dims[1] * dims[2],
        "a reference window matches its dimensions"
    );
    assert_eq!(
        reference.len(),
        moving.len(),
        "a moving window matches the reference window"
    );
    if reference.iter().all(|value| *value == reference[0]) {
        return [0; 3];
    }
    let sizes = dims.map(|value| (2 * value.max(2)).next_power_of_two());
    let mut first = Cube::new(sizes);
    let mut second = Cube::new(sizes);
    fill(&mut first, reference, dims);
    fill(&mut second, moving, dims);
    first.transform(false);
    second.transform(false);
    let dc = first.index([0; 3]);
    first.values[dc] = Complex::default();
    second.values[dc] = Complex::default();
    for index in 0..first.values.len() {
        let product = first.values[index].multiply(second.values[index].conjugate());
        let length = (product.re * product.re + product.im * product.im).sqrt();
        first.values[index] = if length > EPSILON {
            Complex {
                re: product.re / length,
                im: product.im / length,
            }
        } else {
            Complex::default()
        };
    }
    first.transform(true);
    locate(&first, dims)
}

fn fill(cube: &mut Cube, source: &[f32], dims: [usize; 3]) {
    let mut mean = 0.0f64;
    for value in source {
        mean += f64::from(*value);
    }
    mean /= source.len() as f64;
    for z in 0..dims[2] {
        for y in 0..dims[1] {
            for x in 0..dims[0] {
                let taper = [taper(x, dims[0]), taper(y, dims[1]), taper(z, dims[2])];
                let value = (f64::from(source[x + dims[0] * (y + dims[1] * z)]) - mean) as f32;
                let index = cube.index([x, y, z]);
                cube.values[index] = Complex {
                    re: value * taper[0] * taper[1] * taper[2],
                    im: 0.0,
                };
            }
        }
    }
}

fn taper(index: usize, length: usize) -> f32 {
    if length < 3 {
        return 1.0;
    }
    let edge = TAPER * (length - 1) as f32 / 2.0;
    let position = index as f32;
    if position < edge {
        return 0.5 + 0.5 * (std::f32::consts::PI * (position / edge - 1.0)).cos();
    }
    let far = (length - 1) as f32 - position;
    if far < edge {
        return 0.5 + 0.5 * (std::f32::consts::PI * (far / edge - 1.0)).cos();
    }
    1.0
}

fn locate(cube: &Cube, dims: [usize; 3]) -> [i32; 3] {
    let sizes = cube.dims;
    let limits = [0, 1, 2].map(|axis| (dims[axis] / 2).max(1) as isize);
    let mut best = [0usize; 3];
    let mut peak = f32::NEG_INFINITY;
    for z in 0..sizes[2] {
        for y in 0..sizes[1] {
            for x in 0..sizes[0] {
                let value = cube.values[cube.index([x, y, z])].re;
                if value <= peak {
                    continue;
                }
                let signed = [
                    signed_index(x, sizes[0]),
                    signed_index(y, sizes[1]),
                    signed_index(z, sizes[2]),
                ];
                if (0..3).any(|axis| signed[axis].abs() > limits[axis]) {
                    continue;
                }
                peak = value;
                best = [x, y, z];
            }
        }
    }
    [0, 1, 2].map(|axis| {
        let before = sample(cube, best, axis, -1);
        let after = sample(cube, best, axis, 1);
        let curvature = before - 2.0 * peak + after;
        let offset = if curvature.abs() > EPSILON {
            (0.5 * (before - after) / curvature).clamp(-PEAK_LIMIT, PEAK_LIMIT)
        } else {
            0.0
        };
        let displacement = -(signed_index(best[axis], sizes[axis]) as f32 + offset);
        displacement.round() as i32
    })
}

fn signed_index(index: usize, length: usize) -> isize {
    let index = index as isize;
    if index > length as isize / 2 {
        index - length as isize
    } else {
        index
    }
}

fn sample(cube: &Cube, at: [usize; 3], axis: usize, step: isize) -> f32 {
    let mut coordinate = at;
    coordinate[axis] = (at[axis] as isize + step).rem_euclid(cube.dims[axis] as isize) as usize;
    cube.values[cube.index(coordinate)].re
}
