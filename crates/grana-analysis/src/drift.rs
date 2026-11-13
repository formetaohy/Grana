use std::f32::consts::TAU;
use std::ops::Range;

use grana_volume::{Extent, Pyramid, VoxelData};
use grana_warp::{Grid, Warp};

const WINDOW: u32 = 64;

const MINIMUM_WINDOW: u32 = 32;
const TAPER: f32 = 0.5;
const UPSAMPLE: usize = 6;
const UPSAMPLED: usize = 9;
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

pub fn shift(reference: &[f32], moving: &[f32], dims: [usize; 3]) -> [f32; 3] {
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
        return [0.0; 3];
    }
    let sizes = dims.map(|value| (2 * value.max(2)).next_power_of_two());
    let mut reference_spectrum = Cube::new(sizes);
    let mut spectrum = Cube::new(sizes);
    fill(&mut reference_spectrum, reference, dims);
    fill(&mut spectrum, moving, dims);
    reference_spectrum.transform(false);
    spectrum.transform(false);
    let dc = reference_spectrum.index([0; 3]);
    reference_spectrum.values[dc] = Complex::default();
    spectrum.values[dc] = Complex::default();
    for index in 0..spectrum.values.len() {
        let product = reference_spectrum.values[index].multiply(spectrum.values[index].conjugate());
        let length = (product.re * product.re + product.im * product.im).sqrt();
        spectrum.values[index] = if length > EPSILON {
            Complex {
                re: product.re / length,
                im: product.im / length,
            }
        } else {
            Complex::default()
        };
    }
    let mut correlation = Cube::new(sizes);
    correlation.values.copy_from_slice(&spectrum.values);
    correlation.transform(true);
    let peak = locate(&correlation, dims);
    refine(&spectrum, peak)
}

fn locate(cube: &Cube, dims: [usize; 3]) -> [usize; 3] {
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
    best
}

fn refine(spectrum: &Cube, peak: [usize; 3]) -> [f32; 3] {
    let sizes = spectrum.dims;
    let target = [0, 1, 2].map(|axis| signed_index(peak[axis], sizes[axis]) as f32);
    let kernels = [0, 1, 2].map(|axis| kernel(sizes[axis], target[axis]));
    let grid = upsample(spectrum, &kernels);
    let center = ((UPSAMPLED - 1) / 2) as isize;
    let mut best = [0usize; 3];
    let mut peak_power = f32::NEG_INFINITY;
    for z in 0..UPSAMPLED {
        for y in 0..UPSAMPLED {
            for x in 0..UPSAMPLED {
                let value = grid[x + UPSAMPLED * (y + UPSAMPLED * z)];
                let power = value.re * value.re + value.im * value.im;
                if power > peak_power {
                    peak_power = power;
                    best = [x, y, z];
                }
            }
        }
    }
    [0, 1, 2].map(|axis| {
        let index = (best[axis] as isize).clamp(1, UPSAMPLED as isize - 2);
        let power = |step: isize| {
            let mut at = best;
            at[axis] = (index + step) as usize;
            let value = grid[at[0] + UPSAMPLED * (at[1] + UPSAMPLED * at[2])];
            value.re * value.re + value.im * value.im
        };
        let before = power(-1);
        let after = power(1);
        let curvature = before - 2.0 * power(0) + after;
        let offset = if curvature.abs() > EPSILON {
            (0.5 * (before - after) / curvature).clamp(-0.5, 0.5)
        } else {
            0.0
        };
        let position =
            target[axis] + (index - center) as f32 / UPSAMPLE as f32 + offset / UPSAMPLE as f32;
        -position
    })
}

fn kernel(size: usize, target: f32) -> Vec<Complex> {
    let center = (UPSAMPLED as f32 - 1.0) * 0.5;
    let mut weights = vec![Complex::default(); UPSAMPLED * size];
    for step in 0..UPSAMPLED {
        let position = target + (step as f32 - center) / UPSAMPLE as f32;
        for bin in 0..size {
            let frequency = if bin * 2 <= size {
                bin as f32
            } else {
                bin as f32 - size as f32
            };
            let angle = TAU * frequency * position / size as f32;
            weights[step * size + bin] = Complex {
                re: angle.cos(),
                im: angle.sin(),
            };
        }
    }
    weights
}

fn upsample(spectrum: &Cube, kernels: &[Vec<Complex>; 3]) -> Vec<Complex> {
    let [nx, ny, nz] = spectrum.dims;
    let mut stage = vec![Complex::default(); UPSAMPLED * ny * nz];
    for z in 0..nz {
        for y in 0..ny {
            let base = nx * (y + ny * z);
            for step in 0..UPSAMPLED {
                let mut sum = Complex::default();
                let weights = &kernels[0][step * nx..step * nx + nx];
                for (bin, weight) in weights.iter().enumerate() {
                    add(&mut sum, spectrum.values[base + bin], *weight);
                }
                stage[step + UPSAMPLED * (y + ny * z)] = sum;
            }
        }
    }
    let mut lines = vec![Complex::default(); UPSAMPLED * UPSAMPLED * nz];
    for z in 0..nz {
        for x in 0..UPSAMPLED {
            for step in 0..UPSAMPLED {
                let mut sum = Complex::default();
                let weights = &kernels[1][step * ny..step * ny + ny];
                for bin in 0..ny {
                    add(
                        &mut sum,
                        stage[x + UPSAMPLED * (bin + ny * z)],
                        weights[bin],
                    );
                }
                lines[x + UPSAMPLED * (step + UPSAMPLED * z)] = sum;
            }
        }
    }
    let mut grid = vec![Complex::default(); UPSAMPLED * UPSAMPLED * UPSAMPLED];
    for y in 0..UPSAMPLED {
        for x in 0..UPSAMPLED {
            for step in 0..UPSAMPLED {
                let mut sum = Complex::default();
                let weights = &kernels[2][step * nz..step * nz + nz];
                for bin in 0..nz {
                    add(
                        &mut sum,
                        lines[x + UPSAMPLED * (y + UPSAMPLED * bin)],
                        weights[bin],
                    );
                }
                grid[x + UPSAMPLED * (y + UPSAMPLED * step)] = sum;
            }
        }
    }
    grid
}

fn add(sum: &mut Complex, value: Complex, weight: Complex) {
    sum.re += value.re * weight.re - value.im * weight.im;
    sum.im += value.re * weight.im + value.im * weight.re;
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

fn signed_index(index: usize, length: usize) -> isize {
    let index = index as isize;
    if index > length as isize / 2 {
        index - length as isize
    } else {
        index
    }
}

pub const BLOCK: u32 = 64;

const BLOCKS_PER_AXIS: u32 = 4;
const MINIMUM_SAMPLES: usize = 4;
const NOISE_FLOOR: f32 = 0.35;
const REGULARITY: f32 = 0.05;
const ANCHOR: f32 = 0.02;
const RESIDUAL_LIMIT: f32 = 0.5;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Smoothing {
    neighbour: f32,
    anchor: f32,
}

impl Smoothing {
    pub const SMOOTH: Self = Self {
        neighbour: REGULARITY,
        anchor: ANCHOR,
    };

    pub const EXACT: Self = Self {
        neighbour: 0.0,
        anchor: 0.0,
    };

    pub fn new(neighbour: f32, anchor: f32) -> Self {
        assert!(
            [neighbour, anchor]
                .iter()
                .all(|value| value.is_finite() && *value >= 0.0),
            "a displacement smoothing prior weighs non negative couplings"
        );
        Self { neighbour, anchor }
    }

    pub fn neighbour(&self) -> f32 {
        self.neighbour
    }

    pub fn anchor(&self) -> f32 {
        self.anchor
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Sample {
    pub point: [f32; 3],
    pub shift: [f32; 3],
    pub weight: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lattice {
    dims: [u32; 3],
    origin: [f32; 3],
    step: [f32; 3],
}

impl Lattice {
    pub fn covering(bounds: ([f32; 3], [f32; 3]), nodes: [u32; 3]) -> Self {
        let dims = nodes.map(|count| count.max(1));
        let step = [0, 1, 2].map(|axis| {
            let span = bounds.1[axis] - bounds.0[axis];
            if dims[axis] <= 1 || span <= 0.0 {
                1.0
            } else {
                span / (dims[axis] - 1) as f32
            }
        });
        assert!(
            step.iter().all(|value| value.is_finite() && *value > 0.0),
            "a displacement lattice spans a positive world box"
        );
        Self {
            dims,
            origin: bounds.0,
            step,
        }
    }

    pub fn dims(self) -> [u32; 3] {
        self.dims
    }

    pub fn origin(self) -> [f32; 3] {
        self.origin
    }

    pub fn step(self) -> [f32; 3] {
        self.step
    }

    pub fn nodes(self) -> usize {
        (self.dims[0] * self.dims[1] * self.dims[2]) as usize
    }
}

pub struct Blocks {
    pub ranges: Vec<[Range<u32>; 3]>,
    pub counts: [u32; 3],
}

pub fn blocks(extent: Extent, side: u32) -> Blocks {
    let dims = [extent.x(), extent.y(), extent.z()];
    let sizes = [0, 1, 2].map(|axis| side.min(dims[axis]).max(1));
    let counts = [0, 1, 2].map(|axis| (dims[axis] / sizes[axis]).clamp(1, BLOCKS_PER_AXIS));
    let starts = [0, 1, 2].map(|axis| {
        let count = counts[axis];
        let free = dims[axis] - sizes[axis];
        (0..count)
            .map(|index| {
                if count == 1 {
                    free / 2
                } else {
                    index * free / (count - 1)
                }
            })
            .collect::<Vec<u32>>()
    });
    let mut ranges = Vec::with_capacity((counts[0] * counts[1] * counts[2]) as usize);
    for z in &starts[2] {
        for y in &starts[1] {
            for x in &starts[0] {
                ranges.push([*x..*x + sizes[0], *y..*y + sizes[1], *z..*z + sizes[2]]);
            }
        }
    }
    Blocks { ranges, counts }
}

pub fn correlation(reference: &[f32], moving: &[f32]) -> f32 {
    assert_eq!(
        reference.len(),
        moving.len(),
        "a correlation compares windows of one size"
    );
    let count = reference.len() as f64;
    let mean_reference = reference.iter().map(|value| f64::from(*value)).sum::<f64>() / count;
    let mean_moving = moving.iter().map(|value| f64::from(*value)).sum::<f64>() / count;
    let mut cross = 0.0f64;
    let mut square_reference = 0.0f64;
    let mut square_moving = 0.0f64;
    for (left, right) in reference.iter().zip(moving) {
        let left = f64::from(*left) - mean_reference;
        let right = f64::from(*right) - mean_moving;
        cross += left * right;
        square_reference += left * left;
        square_moving += right * right;
    }
    let denominator = (square_reference * square_moving).sqrt();
    if denominator <= 1.0e-12 {
        0.0
    } else {
        (cross / denominator) as f32
    }
}

pub fn spread(samples: &[Sample]) -> f32 {
    let Some(mean) = mean(samples) else {
        return 0.0;
    };
    let mut total = 0.0f32;
    let mut weighed = 0.0f32;
    for sample in samples {
        let offset = [0, 1, 2].map(|axis| sample.shift[axis] - mean[axis]);
        total += sample.weight * offset.iter().map(|value| value * value).sum::<f32>();
        weighed += sample.weight;
    }
    if weighed <= 0.0 {
        0.0
    } else {
        (total / weighed).sqrt()
    }
}

pub fn mean(samples: &[Sample]) -> Option<[f32; 3]> {
    let mut sum = [0.0f32; 3];
    let mut total = 0.0f32;
    for sample in samples.iter().filter(|sample| sample.weight > 0.0) {
        for (sum, shift) in sum.iter_mut().zip(sample.shift) {
            *sum += sample.weight * shift;
        }
        total += sample.weight;
    }
    (total > 0.0).then(|| sum.map(|value| value / total))
}

pub fn deform(
    samples: &[Sample],
    lattice: Lattice,
    scale: f32,
    smoothing: Smoothing,
) -> Option<Warp> {
    if samples.len() < MINIMUM_SAMPLES {
        return None;
    }
    let spread = spread(samples);
    if !(spread.is_finite() && spread > NOISE_FLOOR * scale) {
        return None;
    }
    let mean = mean(samples)?;
    let values = fit(samples, lattice, mean, smoothing)?;
    let grid = Grid::new(lattice.dims, lattice.origin, lattice.step, values);
    let residual = residual(&grid, samples);
    let deviation = grid
        .values()
        .iter()
        .map(|value| {
            let offset = [0, 1, 2].map(|axis| value[axis] - mean[axis]);
            offset.iter().map(|value| value * value).sum::<f32>().sqrt()
        })
        .fold(0.0, f32::max);
    if !residual.is_finite() || residual > RESIDUAL_LIMIT * spread {
        return None;
    }
    if deviation < NOISE_FLOOR * scale {
        return None;
    }
    Some(Warp::field(grid))
}

struct Tap {
    node: [usize; 8],
    weight: [f32; 8],
}

fn taps(lattice: Lattice, point: [f32; 3]) -> Tap {
    let mut node = [0usize; 8];
    let mut weight = [0.0f32; 8];
    let mut low = [0usize; 3];
    let mut high = [0usize; 3];
    let mut fraction = [0.0f32; 3];
    for axis in 0..3 {
        let dims = lattice.dims[axis];
        if dims <= 1 {
            fraction[axis] = 0.0;
            continue;
        }
        let position = ((point[axis] - lattice.origin[axis]) / lattice.step[axis])
            .clamp(0.0, (dims - 1) as f32);
        let base = position.floor() as u32;
        low[axis] = base.min(dims - 2) as usize;
        high[axis] = low[axis] + 1;
        fraction[axis] = position - low[axis] as f32;
    }
    let stride = [
        1,
        lattice.dims[0] as usize,
        lattice.dims[0] as usize * lattice.dims[1] as usize,
    ];
    for corner in 0..8usize {
        let at = [
            if corner & 1 == 0 { low[0] } else { high[0] },
            if corner & 2 == 0 { low[1] } else { high[1] },
            if corner & 4 == 0 { low[2] } else { high[2] },
        ];
        node[corner] = at[0] * stride[0] + at[1] * stride[1] + at[2] * stride[2];
        let mut factor = 1.0f32;
        for (axis, component) in fraction.iter().enumerate() {
            factor *= if (corner >> axis) & 1 == 1 {
                *component
            } else {
                1.0 - *component
            };
        }
        weight[corner] = factor;
    }
    Tap { node, weight }
}

fn fit(
    samples: &[Sample],
    lattice: Lattice,
    mean: [f32; 3],
    smoothing: Smoothing,
) -> Option<Vec<[f32; 3]>> {
    let count = lattice.nodes();
    let dims = lattice.dims;
    let stride = [
        1usize,
        dims[0] as usize,
        dims[0] as usize * dims[1] as usize,
    ];
    let taps = samples
        .iter()
        .map(|sample| taps(lattice, sample.point))
        .collect::<Vec<_>>();
    let total = samples.iter().map(|sample| sample.weight).sum::<f32>();
    let regularity = smoothing.neighbour * total / count as f32;
    let anchor = smoothing.anchor * total / count as f32;
    let mut rhs = vec![[0.0f32; 3]; count];
    for (sample, tap) in samples.iter().zip(&taps) {
        for corner in 0..8usize {
            let weight = sample.weight * tap.weight[corner];
            for (value, shift) in rhs[tap.node[corner]].iter_mut().zip(sample.shift) {
                *value += weight * shift;
            }
        }
    }
    let apply = |field: &[[f32; 3]], target: &mut [[f32; 3]]| {
        target.fill([0.0; 3]);
        for (sample, tap) in samples.iter().zip(&taps) {
            let mut value = [0.0f32; 3];
            for corner in 0..8usize {
                let weight = tap.weight[corner];
                for (value, component) in value.iter_mut().zip(field[tap.node[corner]]) {
                    *value += weight * component;
                }
            }
            for corner in 0..8usize {
                let weight = sample.weight * tap.weight[corner];
                for (target, value) in target[tap.node[corner]].iter_mut().zip(value) {
                    *target += weight * value;
                }
            }
        }
        for node in 0..count {
            let at = [
                node % stride[1],
                (node / stride[1]) % dims[1] as usize,
                node / stride[2],
            ];
            for axis in 0..3 {
                if at[axis] + 1 >= dims[axis] as usize {
                    continue;
                }
                let other = node + stride[axis];
                for component in 0..3 {
                    let delta = regularity * (field[node][component] - field[other][component]);
                    target[node][component] += delta;
                    target[other][component] -= delta;
                }
            }
        }
        for (node, target) in target.iter_mut().enumerate() {
            for (axis, target) in target.iter_mut().enumerate() {
                *target += anchor * (field[node][axis] - mean[axis]);
            }
        }
    };
    let mut matrix = vec![vec![0.0f32; count]; count];
    for column in 0..count {
        let mut unit = vec![[0.0f32; 3]; count];
        unit[column][0] = 1.0;
        let mut out = vec![[0.0f32; 3]; count];
        apply(&unit, &mut out);
        for row in 0..count {
            matrix[row][column] = out[row][0];
        }
    }
    let mut lower = vec![vec![0.0f32; count]; count];
    for row in 0..count {
        for column in 0..=row {
            let mut sum = matrix[row][column];
            for (row_tap, column_tap) in lower[row][..column].iter().zip(&lower[column][..column]) {
                sum -= row_tap * column_tap;
            }
            if row == column {
                if sum <= 0.0 {
                    return None;
                }
                lower[row][row] = sum.sqrt();
            } else {
                lower[row][column] = sum / lower[column][column];
            }
        }
    }
    let mut field = vec![[0.0f32; 3]; count];
    for axis in 0..3 {
        let mut forward = vec![0.0f32; count];
        for row in 0..count {
            let mut sum = rhs[row][axis];
            for column in 0..row {
                sum -= lower[row][column] * forward[column];
            }
            forward[row] = sum / lower[row][row];
        }
        let mut backward = vec![0.0f32; count];
        for row in (0..count).rev() {
            let mut sum = forward[row];
            for column in row + 1..count {
                sum -= lower[column][row] * backward[column];
            }
            backward[row] = sum / lower[row][row];
        }
        for (value, solved) in field.iter_mut().zip(backward) {
            value[axis] = solved;
        }
    }
    Some(field)
}

fn residual(grid: &Grid, samples: &[Sample]) -> f32 {
    let mut total = 0.0f32;
    let mut weighed = 0.0f32;
    for sample in samples.iter().filter(|sample| sample.weight > 0.0) {
        let fitted = grid.sample(sample.point);
        let offset = [0, 1, 2].map(|axis| fitted[axis] - sample.shift[axis]);
        total += sample.weight * offset.iter().map(|value| value * value).sum::<f32>();
        weighed += sample.weight;
    }
    if weighed <= 0.0 {
        f32::INFINITY
    } else {
        (total / weighed).sqrt()
    }
}
