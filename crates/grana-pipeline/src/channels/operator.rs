use std::ops::Range;

use grana_scene::{DerivedSpec, GAUSSIAN_REACH, Operation};
use grana_volume::{ChannelInfo, Level, Spacing, VoxelData};

use super::ChannelsError;

const HALO_LIMIT: u32 = 256;

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
    channels: &[ChannelInfo],
) -> Result<VoxelData, ChannelsError> {
    match spec.operation() {
        Operation::Smooth(smooth) => {
            smooth_channel(spec, reader, ranges, info, smooth.sigma(), channels)
        }
        Operation::Background(_) => background_channel(spec, reader, ranges, info, channels),
        Operation::Ratio(ratio) => {
            ratio_channel(spec, reader, ranges, channels, ratio.denominator())
        }
        Operation::Mask { .. } | Operation::Restore(_) | Operation::Bleach => {
            Err(ChannelsError::Derived {
                detail: format!("{} is not an operator channel", spec.name()),
            })
        }
    }
}

fn channel_info<'a>(
    channels: &'a [ChannelInfo],
    spec: &DerivedSpec,
) -> Result<&'a ChannelInfo, ChannelsError> {
    channels
        .get(spec.source())
        .ok_or_else(|| ChannelsError::Derived {
            detail: format!(
                "the operator channel {} reads a channel of the dataset",
                spec.name()
            ),
        })
}

fn smooth_channel(
    spec: &DerivedSpec,
    reader: &dyn RegionSource,
    ranges: &[Range<u32>; 3],
    info: Level,
    sigma: f32,
    channels: &[ChannelInfo],
) -> Result<VoxelData, ChannelsError> {
    let halo = halo(spec, info)?;
    let extent = info.extent();
    let limits = [extent.x(), extent.y(), extent.z()];
    let expanded = expand(ranges, &halo, &limits);
    let data = reader.read(spec.source(), &expanded)?;
    let dims = dims(&expanded);
    let sigmas = sigma.map_axis(info.spacing(), |sigma, step| sigma / step);
    let kernels = [0, 1, 2].map(|axis| gaussian(sigmas[axis]));
    let source = channel_info(channels, spec)?;
    let mut values = vec![0.0f32; dims[0] * dims[1] * dims[2]];
    data.write_values_absent_as_nan(source.no_data(), &mut values);
    let present = values
        .iter()
        .map(|value| value.is_finite())
        .collect::<Vec<bool>>();
    let mut mass = present
        .iter()
        .map(|present| if *present { 1.0f32 } else { 0.0 })
        .collect::<Vec<f32>>();
    for (axis, kernel) in kernels.iter().enumerate() {
        let (sums, weights) = gaussian_axis(&values, &mass, dims, axis, kernel);
        values = sums;
        mass = weights;
    }
    for (value, present) in values.iter_mut().zip(&present) {
        if !present {
            *value = f32::NAN;
        }
    }
    Ok(crop(&values, &expanded, ranges))
}

fn background_channel(
    spec: &DerivedSpec,
    reader: &dyn RegionSource,
    ranges: &[Range<u32>; 3],
    info: Level,
    channels: &[ChannelInfo],
) -> Result<VoxelData, ChannelsError> {
    let halo = halo(spec, info)?;
    let extent = info.extent();
    let limits = [extent.x(), extent.y(), extent.z()];
    let expanded = expand(ranges, &halo, &limits);
    let data = reader.read(spec.source(), &expanded)?;
    let dims = dims(&expanded);
    let source = channel_info(channels, spec)?;
    let mut values = vec![0.0f32; dims[0] * dims[1] * dims[2]];
    data.write_values_absent_as_nan(source.no_data(), &mut values);
    let present = values
        .iter()
        .map(|value| value.is_finite())
        .collect::<Vec<bool>>();
    let mut mass = present
        .iter()
        .map(|present| if *present { 1.0f32 } else { 0.0 })
        .collect::<Vec<f32>>();
    let mut sums = values.clone();
    for (axis, radius) in halo.iter().enumerate() {
        let (next_sums, next_mass) = box_axis(&sums, &mass, dims, axis, *radius);
        sums = next_sums;
        mass = next_mass;
    }
    for (value, mean) in values.iter_mut().zip(&sums) {
        *value = match value.is_finite() {
            true => (*value - *mean).max(0.0),
            false => f32::NAN,
        };
    }
    Ok(crop(&values, &expanded, ranges))
}

fn ratio_channel(
    spec: &DerivedSpec,
    reader: &dyn RegionSource,
    ranges: &[Range<u32>; 3],
    channels: &[ChannelInfo],
    denominator: usize,
) -> Result<VoxelData, ChannelsError> {
    let dims = dims(ranges);
    let numerator = reader.read(spec.source(), ranges)?;
    let divisor = reader.read(denominator, ranges)?;
    let up = channel_info(channels, spec)?;
    let down = channels
        .get(denominator)
        .ok_or_else(|| ChannelsError::Derived {
            detail: format!(
                "the ratio channel {} divides a channel of the dataset",
                spec.name()
            ),
        })?;
    let mut values = vec![0.0f32; dims[0] * dims[1] * dims[2]];
    let mut divisors = vec![0.0f32; values.len()];
    numerator.write_values_absent_as_nan(up.no_data(), &mut values);
    divisor.write_values_absent_as_nan(down.no_data(), &mut divisors);
    for (value, divisor) in values.iter_mut().zip(&divisors) {
        *value = if value.is_finite() && divisor.is_finite() && *divisor != 0.0 {
            *value / *divisor
        } else {
            f32::NAN
        };
    }
    Ok(VoxelData::F32(values))
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

fn gaussian_axis(
    values: &[f32],
    mass: &[f32],
    dims: [usize; 3],
    axis: usize,
    kernel: &[f32],
) -> (Vec<f32>, Vec<f32>) {
    let line = dims[axis];
    let stride = [1usize, dims[0], dims[0] * dims[1]][axis];
    let outer = [dims[1] * dims[2], dims[0] * dims[2], dims[0] * dims[1]][axis];
    let radius = kernel.len() / 2;
    let mut sums = vec![0.0f32; values.len()];
    let mut weights = vec![0.0f32; values.len()];
    for index in 0..outer {
        let base = line_base(index, axis, dims[0], dims[1]);
        for position in 0..line {
            let mut sum = 0.0f32;
            let mut weight = 0.0f32;
            for (tap, kernel_weight) in kernel.iter().enumerate() {
                let sample = (position as i64 + tap as i64 - radius as i64)
                    .clamp(0, line as i64 - 1) as usize;
                let at = base + sample * stride;
                if mass[at] > 0.0 {
                    sum += kernel_weight * values[at];
                    weight += kernel_weight;
                }
            }
            let at = base + position * stride;
            sums[at] = if weight > 0.0 { sum / weight } else { f32::NAN };
            weights[at] = weight;
        }
    }
    (sums, weights)
}

fn box_axis(
    sums: &[f32],
    mass: &[f32],
    dims: [usize; 3],
    axis: usize,
    radius: u32,
) -> (Vec<f32>, Vec<f32>) {
    let line = dims[axis];
    let stride = [1usize, dims[0], dims[0] * dims[1]][axis];
    let outer = [dims[1] * dims[2], dims[0] * dims[2], dims[0] * dims[1]][axis];
    let reach = (radius as usize).min(line - 1);
    let mut value_prefix = vec![0.0f64; line + 1];
    let mut mass_prefix = vec![0.0f64; line + 1];
    let mut means = vec![0.0f32; sums.len()];
    let mut weights = vec![0.0f32; mass.len()];
    for index in 0..outer {
        let base = line_base(index, axis, dims[0], dims[1]);
        for position in 0..line {
            let at = base + position * stride;
            let held = match mass[at] > 0.0 {
                true => sums[at],
                false => 0.0,
            };
            value_prefix[position + 1] = value_prefix[position] + f64::from(held);
            mass_prefix[position + 1] = mass_prefix[position] + f64::from(mass[at]);
        }
        for position in 0..line {
            let low = position.saturating_sub(reach);
            let high = (position + reach + 1).min(line);
            let weight = mass_prefix[high] - mass_prefix[low];
            let at = base + position * stride;
            means[at] = if weight > 0.0 {
                ((value_prefix[high] - value_prefix[low]) / weight) as f32
            } else {
                f32::NAN
            };
            weights[at] = weight as f32;
        }
    }
    (means, weights)
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

#[cfg(test)]
mod tests {
    use super::*;
    use grana_volume::{
        Extent, Intensity, NoData, Statistics, VoxelData, VoxelFormat, VoxelSemantics,
    };

    struct Source;

    impl RegionSource for Source {
        fn read(
            &self,
            channel: usize,
            ranges: &[std::ops::Range<u32>; 3],
        ) -> Result<VoxelData, ChannelsError> {
            let dims = dims(ranges);
            let mut values = vec![2.0f32; dims[0] * dims[1] * dims[2]];
            for z in ranges[2].clone() {
                for y in ranges[1].clone() {
                    for x in ranges[0].clone() {
                        let index = (x - ranges[0].start) as usize
                            + dims[0]
                                * ((y - ranges[1].start) as usize
                                    + dims[1] * (z - ranges[2].start) as usize);
                        if x == 0 && y == 0 && z == 0 {
                            values[index] = -1.0;
                        } else if channel == 1 && x == 1 && y == 0 && z == 0 {
                            values[index] = 0.0;
                        }
                    }
                }
            }
            Ok(VoxelData::F32(values))
        }
    }

    fn channel(name: &str) -> ChannelInfo {
        ChannelInfo::new(
            name,
            [1.0, 1.0, 1.0],
            Statistics::declared(Intensity::new(0.0, 4.0), VoxelSemantics::Scalar),
            VoxelFormat::F32,
        )
        .with_no_data(NoData::F32(-1.0))
    }

    #[test]
    fn keeps_missing_voxels_out_of_smoothed_and_ratio_channels() {
        let extent = Extent::new(24, 24, 24);
        let info = Level::new(extent, Spacing::new(1.0, 1.0, 1.0));
        let source = Source;
        let channels = [channel("signal"), channel("divisor")];
        let ranges = [0..24, 0..24, 0..24];
        let at = |values: &[f32], x: usize, y: usize, z: usize| values[x + 24 * (y + 24 * z)];

        let smooth = DerivedSpec::smoothed(
            0,
            "smoothed",
            [1.0, 1.0, 1.0],
            Intensity::new(0.0, 4.0),
            2.0,
        );
        let data = read(&smooth, &source, &ranges, info, &channels).expect("the smoothing reads");
        let VoxelData::F32(values) = data else {
            panic!("an operator writes f32");
        };
        assert!(
            at(&values, 0, 0, 0).is_nan(),
            "an absent voxel stays absent"
        );
        assert!(
            (at(&values, 8, 8, 8) - 2.0).abs() < 1.0e-6,
            "the fill never enters a valid neighbourhood"
        );
        assert!(
            (at(&values, 1, 0, 0) - 2.0).abs() < 1.0e-5,
            "a valid voxel beside an absent one keeps its own value"
        );

        let ratio = DerivedSpec::ratio(0, 1, "ratio", [1.0, 1.0, 1.0], Intensity::new(0.0, 2.0));
        let data = read(&ratio, &source, &ranges, info, &channels).expect("the ratio reads");
        let VoxelData::F32(values) = data else {
            panic!("an operator writes f32");
        };
        assert!(
            at(&values, 0, 0, 0).is_nan(),
            "an absent divisor stays absent"
        );
        assert!(
            at(&values, 1, 0, 0).is_nan(),
            "a zero divisor is absent rather than zero"
        );
        assert!(
            (at(&values, 8, 8, 8) - 1.0).abs() < 1.0e-6,
            "a valid ratio divides its two channels"
        );
    }
}
