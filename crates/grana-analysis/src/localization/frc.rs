use super::fourier;
use super::grid::Grid;
use super::report::Frc;

const LIMIT: usize = 2048;

const ROBUSTNESS: usize = 3;

const SPAN_MINIMUM: usize = 5;

pub(super) fn measure(first: &Grid, second: &Grid, scoped: bool) -> Frc {
    let size = first.size();
    assert_eq!(
        size,
        second.size(),
        "a Fourier ring correlation pairs images of one size"
    );
    assert_eq!(
        size[0], size[1],
        "a Fourier ring correlation pairs square images"
    );
    let side = size[0] as usize;
    assert!(
        side.is_power_of_two() && side <= LIMIT,
        "a Fourier ring correlation runs on a bounded power of two"
    );
    let rings = side / 2 + 1;
    let mask = taper(side);
    let mut first_real = vec![0.0f32; side * side];
    let mut first_imaginary = vec![0.0f32; side * side];
    let mut second_real = vec![0.0f32; side * side];
    let mut second_imaginary = vec![0.0f32; side * side];
    for row in 0..side {
        for column in 0..side {
            let index = column + row * side;
            let weight = mask[column] * mask[row];
            first_real[index] = first.values()[index] * weight;
            second_real[index] = second.values()[index] * weight;
        }
    }
    fourier::transform(&mut first_real, &mut first_imaginary, side);
    fourier::transform(&mut second_real, &mut second_imaginary, side);
    let mut product = vec![0.0f64; rings];
    let mut power_first = vec![0.0f64; rings];
    let mut power_second = vec![0.0f64; rings];
    for row in 0..side {
        let vertical = fold(row, side);
        for column in 0..side {
            let horizontal = fold(column, side);
            let radius = ((horizontal * horizontal + vertical * vertical) as f64).sqrt();
            let ring = radius as usize;
            if ring >= rings {
                continue;
            }
            let index = column + row * side;
            let (a, b) = (first_real[index], first_imaginary[index]);
            let (c, d) = (second_real[index], second_imaginary[index]);
            product[ring] += f64::from(a * c + b * d);
            power_first[ring] += f64::from(a * a + b * b);
            power_second[ring] += f64::from(c * c + d * d);
        }
    }
    let pixel = f64::from(first.pixel()) * 1000.0;
    let frequencies = (0..rings)
        .map(|ring| (ring as f64 / (side as f64 * pixel)) as f32)
        .collect::<Vec<_>>();
    let curve = (0..rings)
        .map(|ring| {
            let denominator = (power_first[ring] * power_second[ring]).sqrt();
            if denominator > 0.0 {
                (product[ring] / denominator) as f32
            } else {
                0.0
            }
        })
        .collect::<Vec<_>>();
    let span = ((side / 2).div_ceil(20)).max(SPAN_MINIMUM);
    let smooth = smooth(&curve, span);
    let resolution = crossing(&frequencies, &smooth);
    Frc::new(frequencies, curve, smooth, resolution, scoped)
}

fn fold(index: usize, size: usize) -> i64 {
    if index <= size / 2 {
        index as i64
    } else {
        index as i64 - size as i64
    }
}

fn taper(size: usize) -> Vec<f32> {
    let reach = 8.0f64;
    (0..size)
        .map(|index| {
            let position = (index as f64 - size as f64 / 2.0) / size as f64;
            if position.abs() < (reach - 2.0) / (reach * 2.0) {
                1.0
            } else {
                0.5 - 0.5 * (std::f64::consts::PI * reach * position).cos()
            }
        })
        .map(|value| value as f32)
        .collect()
}

fn smooth(values: &[f32], span: usize) -> Vec<f32> {
    let count = values.len();
    if count < SPAN_MINIMUM {
        return values.to_vec();
    }
    let window = span.clamp(3, count);
    let mut robust = vec![1.0f64; count];
    let mut smoothed = vec![0.0f32; count];
    for _ in 0..=ROBUSTNESS {
        for (index, value) in smoothed.iter_mut().enumerate() {
            let half = window / 2;
            let start = index.saturating_sub(half).min(count - window);
            let end = start + window;
            let reach = (start..end)
                .map(|point| (point as f64 - index as f64).abs())
                .fold(0.0f64, f64::max);
            if reach <= 0.0 {
                *value = values[index];
                continue;
            }
            let [total, moment, ordinate, spread, covariance] = (start..end).fold(
                [0.0f64; 5],
                |[total, moment, ordinate, spread, covariance], point| {
                    let offset = point as f64 - index as f64;
                    let ratio = offset.abs() / reach;
                    let tricube = (1.0 - ratio.powi(3)).powi(3);
                    let weight = tricube * robust[point];
                    let held = f64::from(values[point]);
                    [
                        total + weight,
                        moment + weight * offset,
                        ordinate + weight * held,
                        spread + weight * offset * offset,
                        covariance + weight * offset * held,
                    ]
                },
            );
            let determinant = total * spread - moment * moment;
            let fitted = if determinant.abs() > 1.0e-12 {
                (ordinate * spread - moment * covariance) / determinant
            } else if total > 0.0 {
                ordinate / total
            } else {
                f64::from(values[index])
            };
            *value = fitted as f32;
        }
        let mut residuals = (0..count)
            .map(|index| (f64::from(values[index]) - f64::from(smoothed[index])).abs())
            .collect::<Vec<_>>();
        residuals.sort_by(|left, right| left.total_cmp(right));
        let deviation = residuals[count / 2];
        if deviation.is_nan() || deviation <= 0.0 {
            break;
        }
        for (index, robust) in robust.iter_mut().enumerate() {
            let ratio = (f64::from(values[index]) - f64::from(smoothed[index])) / (6.0 * deviation);
            *robust = if ratio.abs() < 1.0 {
                (1.0 - ratio * ratio).powi(2)
            } else {
                0.0
            };
        }
    }
    smoothed
}

fn crossing(frequencies: &[f32], values: &[f32]) -> Option<f32> {
    let threshold = Frc::THRESHOLD;
    for index in 1..values.len().min(frequencies.len()) {
        if values[index - 1] >= threshold && values[index] < threshold {
            let low = f64::from(frequencies[index - 1]);
            let high = f64::from(frequencies[index]);
            let from = f64::from(values[index - 1]);
            let to = f64::from(values[index]);
            let span = to - from;
            if span.abs() <= f64::EPSILON {
                continue;
            }
            let crossing = low + (f64::from(threshold) - from) * (high - low) / span;
            if crossing > 0.0 {
                return Some((1.0 / crossing) as f32);
            }
        }
    }
    None
}
