use crate::drift;

const GRID: usize = 256;
const MINIMUM_SPAN: f32 = 1.0e-3;
const SEGMENT_TARGET: usize = 100;
const SEGMENT_LIMIT: usize = 16;
const MINIMUM_POINTS: usize = 100;
const WEIGHT_FLOOR: f32 = 1.0e-3;

pub fn localization_drift(
    frames: &[Vec<[f32; 3]>],
    reference: u32,
    report: &mut dyn FnMut(usize, usize) -> bool,
) -> Result<Option<Vec<[f32; 3]>>, String> {
    if frames.len() < 2 {
        return Err("the object set holds one frame to align".to_string());
    }
    if reference as usize >= frames.len() {
        return Err("the drift reference lies outside the time series".to_string());
    }
    let mut low = [f32::INFINITY; 2];
    let mut high = [f32::NEG_INFINITY; 2];
    let mut total = 0usize;
    for frame in frames {
        for point in frame {
            for axis in 0..2 {
                low[axis] = low[axis].min(point[axis]);
                high[axis] = high[axis].max(point[axis]);
            }
        }
        total += frame.len();
    }
    if total < MINIMUM_POINTS {
        return Err(format!(
            "the object set holds {total} localizations, too few to align"
        ));
    }
    let span = (high[0] - low[0]).max(high[1] - low[1]);
    if !(span.is_finite() && span > MINIMUM_SPAN) {
        return Err("the localizations span no field to align".to_string());
    }
    let bin = span / GRID as f32;
    let mut segments = segmentation(frames, bin, low);
    segments.retain(|segment| !segment.density.is_empty());
    if segments.len() < 2 {
        return Err("the localizations fill fewer than two segments to align".to_string());
    }
    let pairs = segments.len() * (segments.len() - 1) / 2;
    let shifts = solve(&segments, bin, pairs, report);
    let Some(shifts) = shifts else {
        return Ok(None);
    };
    let offsets = interpolate(&segments, &shifts, frames.len());
    let anchor = offsets[reference as usize];
    if !anchor.iter().all(|value| value.is_finite()) {
        return Err("the drift estimate carries a non-finite offset".to_string());
    }
    Ok(Some(
        offsets
            .into_iter()
            .map(|offset| [offset[0] - anchor[0], offset[1] - anchor[1], 0.0])
            .collect(),
    ))
}

struct Segment {
    center: f32,
    density: Vec<f32>,
}

fn segmentation(frames: &[Vec<[f32; 3]>], bin: f32, low: [f32; 2]) -> Vec<Segment> {
    let segments = frames
        .len()
        .div_ceil(SEGMENT_TARGET)
        .clamp(2, SEGMENT_LIMIT)
        .min(frames.len());
    let length = frames.len().div_ceil(segments);
    let mut held = Vec::new();
    for chunk in frames.chunks(length) {
        let mut density = vec![0.0f32; GRID * GRID];
        let mut center = 0.0f64;
        let mut filled = 0usize;
        let start = held.len() * length;
        for (offset, frame) in chunk.iter().enumerate() {
            center += (start + offset) as f64;
            for point in frame {
                let x = ((point[0] - low[0]) / bin).floor();
                let y = ((point[1] - low[1]) / bin).floor();
                if !(x.is_finite() && y.is_finite()) {
                    continue;
                }
                let x = (x as isize).clamp(0, GRID as isize - 1) as usize;
                let y = (y as isize).clamp(0, GRID as isize - 1) as usize;
                density[x + GRID * y] += 1.0;
                filled += 1;
            }
        }
        held.push(Segment {
            center: if chunk.is_empty() {
                start as f32
            } else {
                (center / chunk.len() as f64) as f32
            },
            density: if filled == 0 { Vec::new() } else { density },
        });
    }
    held
}

fn solve(
    segments: &[Segment],
    bin: f32,
    pairs: usize,
    report: &mut dyn FnMut(usize, usize) -> bool,
) -> Option<Vec<[f32; 3]>> {
    let count = segments.len();
    let unknowns = count - 1;
    let mut matrix = vec![vec![0.0f64; unknowns]; unknowns];
    let mut right = vec![[0.0f64; 2]; unknowns];
    let mut done = 0usize;
    for first in 0..count {
        for second in 0..first {
            done += 1;
            if !report(done, pairs) {
                return None;
            }
            let reference = &segments[second].density;
            let moving = &segments[first].density;
            let shift = drift::shift(reference, moving, [GRID, GRID, 1]);
            let correlation = drift::correlation(reference, moving);
            let weight = f64::from(correlation.max(0.0) + WEIGHT_FLOOR);
            let delta = [f64::from(shift[0]), f64::from(shift[1])];
            if first > 0 {
                matrix[first - 1][first - 1] += weight;
            }
            if second > 0 {
                matrix[second - 1][second - 1] += weight;
            }
            if first > 0 && second > 0 {
                matrix[first - 1][second - 1] -= weight;
                matrix[second - 1][first - 1] -= weight;
            }
            if first > 0 {
                for axis in 0..2 {
                    right[first - 1][axis] += weight * delta[axis];
                }
            }
            if second > 0 {
                for axis in 0..2 {
                    right[second - 1][axis] -= weight * delta[axis];
                }
            }
        }
    }
    let mut shifts = vec![[0.0f32; 3]; count];
    let horizontal = eliminate(&matrix, &right, 0)?;
    let vertical = eliminate(&matrix, &right, 1)?;
    for (index, (x, y)) in horizontal.into_iter().zip(vertical).enumerate() {
        shifts[index + 1] = [
            (x * f64::from(bin)) as f32,
            (y * f64::from(bin)) as f32,
            0.0,
        ];
    }
    Some(shifts)
}

fn eliminate(matrix: &[Vec<f64>], right: &[[f64; 2]], axis: usize) -> Option<Vec<f64>> {
    let size = matrix.len();
    let mut rows = matrix.to_vec();
    let mut values = (0..size)
        .map(|index| right[index][axis])
        .collect::<Vec<_>>();
    for column in 0..size {
        let pivot = (column..size).max_by(|left, right| {
            rows[*left][column]
                .abs()
                .total_cmp(&rows[*right][column].abs())
        })?;
        if rows[pivot][column].abs() <= f64::EPSILON {
            return None;
        }
        rows.swap(column, pivot);
        values.swap(column, pivot);
        for row in column + 1..size {
            let factor = rows[row][column] / rows[column][column];
            if factor == 0.0 {
                continue;
            }
            let pivot = rows[column].clone();
            for (slot, value) in rows[row].iter_mut().enumerate().skip(column) {
                *value -= factor * pivot[slot];
            }
            values[row] -= factor * values[column];
        }
    }
    let mut solution = vec![0.0f64; size];
    for row in (0..size).rev() {
        let mut sum = values[row];
        for column in row + 1..size {
            sum -= rows[row][column] * solution[column];
        }
        solution[row] = sum / rows[row][row];
    }
    solution
        .iter()
        .all(|value| value.is_finite())
        .then_some(solution)
}

fn interpolate(segments: &[Segment], shifts: &[[f32; 3]], frames: usize) -> Vec<[f32; 3]> {
    let mut offsets = Vec::with_capacity(frames);
    for time in 0..frames {
        let time = time as f32;
        let sample = if time <= segments[0].center {
            shifts[0]
        } else if time >= segments[segments.len() - 1].center {
            shifts[segments.len() - 1]
        } else {
            let mut found = 0usize;
            while found + 1 < segments.len() && segments[found + 1].center <= time {
                found += 1;
            }
            let left = segments[found].center;
            let right = segments[found + 1].center;
            let weight = if right > left {
                ((time - left) / (right - left)).clamp(0.0, 1.0)
            } else {
                0.0
            };
            [0, 1, 2].map(|axis| {
                shifts[found][axis] + weight * (shifts[found + 1][axis] - shifts[found][axis])
            })
        };
        offsets.push(sample);
    }
    offsets
}
