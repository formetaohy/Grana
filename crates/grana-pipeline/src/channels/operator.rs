use std::ops::Range;

use grana_scene::{DerivedSpec, Operation};
use grana_volume::{Level, Spacing, VoxelData};

use super::ChannelsError;

const HALO_LIMIT: u32 = 256;
const GAUSSIAN_REACH: f32 = 3.0;

pub(super) trait RegionSource {
    fn read(&self, channel: usize, ranges: &[Range<u32>; 3]) -> Result<VoxelData, ChannelsError>;
}

pub(super) fn halo(spec: &DerivedSpec, info: Level) -> Result<[u32; 3], ChannelsError> {
    let extent = info.extent();
    let limits = [extent.x(), extent.y(), extent.z()];
    let spacing = info.spacing();
    let result = match spec.operation() {
        Operation::Smooth(smooth) => smooth
            .sigma()
            .map_axis(spacing, |sigma, step| GAUSSIAN_REACH * sigma / step)
            .map(|radius| radius.ceil() as u32),
        Operation::Background(background) => background
            .radius()
            .map_axis(spacing, |radius, step| radius / step)
            .map(|radius| (radius.round() as u32).max(1)),
        Operation::Ratio(_)
        | Operation::Mask { .. }
        | Operation::Restore(_)
        | Operation::Bleach => [0; 3],
    };
    for axis in 0..3 {
        if result[axis] > HALO_LIMIT || result[axis] >= limits[axis] {
            return Err(ChannelsError::Derived {
                detail: format!(
                    "the operator channel {} needs a halo wider than its volume",
                    spec.name()
                ),
            });
        }
    }
    Ok(result)
}

pub(super) fn read(
    spec: &DerivedSpec,
    reader: &dyn RegionSource,
    ranges: &[Range<u32>; 3],
    info: Level,
) -> Result<VoxelData, ChannelsError> {
    match spec.operation() {
        Operation::Smooth(smooth) => smooth_channel(spec, reader, ranges, info, smooth.sigma()),
        Operation::Background(_) => background_channel(spec, reader, ranges, info),
        Operation::Ratio(ratio) => ratio_channel(spec, reader, ranges, ratio.denominator()),
        Operation::Mask { .. } | Operation::Restore(_) | Operation::Bleach => {
            Err(ChannelsError::Derived {
                detail: format!("{} is not an operator channel", spec.name()),
            })
        }
    }
}

fn smooth_channel(
    spec: &DerivedSpec,
    reader: &dyn RegionSource,
    ranges: &[Range<u32>; 3],
    info: Level,
    sigma: f32,
) -> Result<VoxelData, ChannelsError> {
    let halo = halo(spec, info)?;
    let extent = info.extent();
    let limits = [extent.x(), extent.y(), extent.z()];
    let expanded = expand(ranges, &halo, &limits);
    let data = reader.read(spec.source(), &expanded)?;
    let dims = dims(&expanded);
    let sigmas = sigma.map_axis(info.spacing(), |sigma, step| sigma / step);
    let kernels = [0, 1, 2].map(|axis| gaussian(sigmas[axis]));
    let mut values = values(&data, &dims);
    let mut scratch = vec![0.0f32; values.len()];
    for (axis, kernel) in kernels.iter().enumerate() {
        gaussian_axis(&values, &mut scratch, dims, axis, kernel);
        std::mem::swap(&mut values, &mut scratch);
    }
    Ok(crop(&values, &expanded, ranges))
}

fn background_channel(
    spec: &DerivedSpec,
    reader: &dyn RegionSource,
    ranges: &[Range<u32>; 3],
    info: Level,
) -> Result<VoxelData, ChannelsError> {
    let halo = halo(spec, info)?;
    let extent = info.extent();
    let limits = [extent.x(), extent.y(), extent.z()];
    let expanded = expand(ranges, &halo, &limits);
    let data = reader.read(spec.source(), &expanded)?;
    let dims = dims(&expanded);
    let radii = halo;
    let mut background = values(&data, &dims);
    let mut scratch = vec![0.0f32; background.len()];
    for (axis, radius) in radii.iter().enumerate() {
        box_axis(&background, &mut scratch, dims, axis, *radius);
        std::mem::swap(&mut background, &mut scratch);
    }
    let mut values = values(&data, &dims);
    for (value, mean) in values.iter_mut().zip(&background) {
        *value = (*value - *mean).max(0.0);
    }
    Ok(crop(&values, &expanded, ranges))
}

fn ratio_channel(
    spec: &DerivedSpec,
    reader: &dyn RegionSource,
    ranges: &[Range<u32>; 3],
    denominator: usize,
) -> Result<VoxelData, ChannelsError> {
    let numerator = reader.read(spec.source(), ranges)?;
    let divisor = reader.read(denominator, ranges)?;
    let dims = dims(ranges);
    let mut up = values(&numerator, &dims);
    let down = values(&divisor, &dims);
    for (value, divisor) in up.iter_mut().zip(&down) {
        *value = if *divisor > 0.0 {
            *value / *divisor
        } else {
            0.0
        };
    }
    Ok(VoxelData::F32(up))
}

trait AxisValue {
    fn map_axis(self, spacing: Spacing, map: impl Fn(f32, f32) -> f32) -> [f32; 3];
}

impl AxisValue for f32 {
    fn map_axis(self, spacing: Spacing, map: impl Fn(f32, f32) -> f32) -> [f32; 3] {
        [spacing.x(), spacing.y(), spacing.z()].map(|step| map(self, step))
    }
}

fn expand(ranges: &[Range<u32>; 3], by: &[u32; 3], limits: &[u32; 3]) -> [Range<u32>; 3] {
    [0, 1, 2].map(|axis| {
        ranges[axis].start.saturating_sub(by[axis])..(ranges[axis].end + by[axis]).min(limits[axis])
    })
}

fn dims(ranges: &[Range<u32>; 3]) -> [usize; 3] {
    [
        (ranges[0].end - ranges[0].start) as usize,
        (ranges[1].end - ranges[1].start) as usize,
        (ranges[2].end - ranges[2].start) as usize,
    ]
}

fn values(data: &VoxelData, dims: &[usize; 3]) -> Vec<f32> {
    let count = dims[0] * dims[1] * dims[2];
    assert_eq!(
        data.voxel_count(),
        count,
        "the operator reads every voxel of its region"
    );
    let mut values = vec![0.0f32; count];
    data.write_values(&mut values);
    values
}

fn crop(values: &[f32], from: &[Range<u32>; 3], to: &[Range<u32>; 3]) -> VoxelData {
    let src = dims(from);
    let dst = dims(to);
    let mut output = Vec::with_capacity(dst[0] * dst[1] * dst[2]);
    for z in to[2].clone() {
        for y in to[1].clone() {
            let row = (z - from[2].start) as usize * src[0] * src[1]
                + (y - from[1].start) as usize * src[0];
            let start = row + (to[0].start - from[0].start) as usize;
            output.extend_from_slice(&values[start..start + dst[0]]);
        }
    }
    VoxelData::F32(output)
}

fn gaussian(sigma: f32) -> Vec<f32> {
    if sigma <= 0.0 {
        return vec![1.0];
    }
    let radius = (GAUSSIAN_REACH * sigma).ceil().max(1.0) as usize;
    let scale = -0.5 / (f64::from(sigma) * f64::from(sigma));
    let mut kernel = (0..=radius)
        .map(|offset| {
            let offset = offset as f64;
            (scale * offset * offset).exp() as f32
        })
        .collect::<Vec<f32>>();
    let mut total = f64::from(kernel[0]);
    for value in kernel.iter().skip(1).take(radius) {
        total += 2.0 * f64::from(*value);
    }
    for value in &mut kernel {
        *value /= total as f32;
    }
    let mut symmetric = Vec::with_capacity(2 * radius + 1);
    for index in (1..=radius).rev() {
        symmetric.push(kernel[index]);
    }
    symmetric.extend_from_slice(&kernel);
    symmetric
}

fn gaussian_axis(values: &[f32], out: &mut [f32], dims: [usize; 3], axis: usize, kernel: &[f32]) {
    let [width, height, depth] = dims;
    let radius = kernel.len() / 2;
    let line = dims[axis];
    let stride = [1usize, width, width * height][axis];
    let outer = [height * depth, width * depth, width * height][axis];
    for index in 0..outer {
        let base = line_base(index, axis, width, height);
        for position in 0..line {
            let mut sum = 0.0f32;
            for (tap, weight) in kernel.iter().enumerate() {
                let sample = position as i64 + tap as i64 - radius as i64;
                let clamped = sample.clamp(0, line as i64 - 1) as usize;
                sum += *weight * values[base + clamped * stride];
            }
            out[base + position * stride] = sum;
        }
    }
}

fn box_axis(values: &[f32], out: &mut [f32], dims: [usize; 3], axis: usize, radius: u32) {
    let [width, height, depth] = dims;
    let line = dims[axis];
    let stride = [1usize, width, width * height][axis];
    let outer = [height * depth, width * depth, width * height][axis];
    let reach = (radius as usize).min(line - 1);
    let mut prefix = vec![0.0f64; line + 1];
    for index in 0..outer {
        let base = line_base(index, axis, width, height);
        for position in 0..line {
            prefix[position + 1] = prefix[position] + f64::from(values[base + position * stride]);
        }
        for position in 0..line {
            let low = position.saturating_sub(reach);
            let high = (position + reach + 1).min(line);
            let count = high - low;
            out[base + position * stride] = (prefix[high] - prefix[low]) as f32 / count as f32;
        }
    }
}

fn line_base(index: usize, axis: usize, width: usize, height: usize) -> usize {
    match axis {
        0 => {
            let y = index % height;
            let z = index / height;
            y * width + z * width * height
        }
        1 => {
            let x = index % width;
            let z = index / width;
            x + z * width * height
        }
        _ => index,
    }
}
