use grana_warp::Warp;

use crate::drift::{self, Lattice, Sample};

const BINS: usize = 64;
const NODES: u32 = 4;
const MINIMUM_BEADS: usize = 3;
const MINIMUM_PAIRS: usize = 3;
const REACH_FRACTION: f32 = 0.75;
const TRIM: f32 = 4.0;
const NEIGHBOURHOOD: isize = 2;

pub struct Fiducial {
    warp: Warp,
    pairs: usize,
    residual: f32,
}

impl Fiducial {
    pub fn warp(&self) -> &Warp {
        &self.warp
    }

    pub fn pairs(&self) -> usize {
        self.pairs
    }

    pub fn residual(&self) -> f32 {
        self.residual
    }
}

pub fn fiducial_registration(
    reference: &[[f32; 3]],
    moving: &[[f32; 3]],
    scale: f32,
) -> Result<Fiducial, String> {
    assert!(
        scale.is_finite() && scale > 0.0,
        "a fiducial registration knows the scale of its beads"
    );
    if reference.len() < MINIMUM_BEADS || moving.len() < MINIMUM_BEADS {
        return Err(format!(
            "a fiducial registration reads at least {MINIMUM_BEADS} beads per channel and the table holds {} and {}",
            reference.len(),
            moving.len()
        ));
    }
    let (low, high) = bounds(reference, moving)?;
    let shift = coarse(low, high, reference, moving)?;
    let reach = reach(reference, low, high);
    let pairs = pairs(reference, moving, low, high, shift, reach.max(scale));
    if pairs.len() < MINIMUM_PAIRS {
        return Err(format!(
            "the beads of the two channels share {} positions within {:.2} µm to register",
            pairs.len(),
            reach.max(scale)
        ));
    }
    let lattice = lattice(low, high);
    let mut samples = pairs
        .iter()
        .map(|(reference, moving)| Sample {
            point: *moving,
            shift: [0, 1, 2].map(|axis| moving[axis] - reference[axis]),
            weight: 1.0,
        })
        .collect::<Vec<_>>();
    let trimmed = trim(&mut samples, lattice, scale);
    let warp =
        drift::deform(&samples, lattice, scale, drift::Smoothing::EXACT).unwrap_or_else(|| {
            let mean = drift::mean(&samples).expect("the kept pairs carry a displacement");
            Warp::shift(mean)
        });
    let residual = samples
        .iter()
        .map(|sample| {
            let offset = [0, 1, 2].map(|axis| sample.shift[axis] - warp.sample(sample.point)[axis]);
            offset.iter().map(|value| value * value).sum::<f32>()
        })
        .sum::<f32>()
        / samples.len() as f32;
    Ok(Fiducial {
        warp,
        pairs: trimmed,
        residual: residual.sqrt(),
    })
}

fn bounds(reference: &[[f32; 3]], moving: &[[f32; 3]]) -> Result<([f32; 3], [f32; 3]), String> {
    let mut low = [f32::INFINITY; 3];
    let mut high = [f32::NEG_INFINITY; 3];
    for point in reference.iter().chain(moving) {
        if point.iter().any(|value| !value.is_finite()) {
            return Err("a fiducial registration reads finite bead positions".to_string());
        }
        for (axis, value) in point.iter().enumerate() {
            low[axis] = low[axis].min(*value);
            high[axis] = high[axis].max(*value);
        }
    }
    Ok((low, high))
}

fn coarse(
    low: [f32; 3],
    high: [f32; 3],
    reference: &[[f32; 3]],
    moving: &[[f32; 3]],
) -> Result<[f32; 3], String> {
    let span = [0, 1, 2].map(|axis| high[axis] - low[axis]);
    let longest = span.iter().copied().fold(0.0f32, f32::max);
    if !(longest.is_finite() && longest > 0.0) {
        return Err("the beads span no field to register".to_string());
    }
    let dims = [0, 1, 2]
        .map(|axis| ((span[axis] / longest * BINS as f32).round() as usize).clamp(1, BINS));
    let step = [0, 1, 2].map(|axis| {
        if dims[axis] <= 1 {
            1.0
        } else {
            span[axis] / (dims[axis] - 1) as f32
        }
    });
    let first = density(reference, low, dims, step);
    let second = density(moving, low, dims, step);
    let found = drift::shift(&first, &second, dims);
    Ok([0, 1, 2].map(|axis| {
        if dims[axis] <= 1 {
            0.0
        } else {
            found[axis] * step[axis]
        }
    }))
}

fn density(points: &[[f32; 3]], low: [f32; 3], dims: [usize; 3], step: [f32; 3]) -> Vec<f32> {
    let mut bins = vec![0.0f32; dims[0] * dims[1] * dims[2]];
    for point in points {
        let bin = [0, 1, 2].map(|axis| {
            (((point[axis] - low[axis]) / step[axis]).round() as isize)
                .clamp(0, dims[axis] as isize - 1) as usize
        });
        bins[bin[0] + dims[0] * (bin[1] + dims[1] * bin[2])] += 1.0;
    }
    bins
}

fn reach(reference: &[[f32; 3]], low: [f32; 3], high: [f32; 3]) -> f32 {
    let span = [0, 1, 2].map(|axis| high[axis] - low[axis]);
    let mut sorted = span;
    sorted.sort_by(f32::total_cmp);
    let area = sorted[1] * sorted[2];
    if area > 0.0 {
        REACH_FRACTION * (area / reference.len() as f32).sqrt()
    } else {
        REACH_FRACTION * sorted[2] / reference.len() as f32
    }
}

fn pairs(
    reference: &[[f32; 3]],
    moving: &[[f32; 3]],
    low: [f32; 3],
    high: [f32; 3],
    shift: [f32; 3],
    reach: f32,
) -> Vec<([f32; 3], [f32; 3])> {
    let span = [0, 1, 2].map(|axis| (high[axis] - low[axis]).max(reach));
    let cells = [0, 1, 2].map(|axis| (span[axis] / reach).ceil().max(1.0) as isize);
    let cell = |point: &[f32; 3]| {
        [0, 1, 2].map(|axis| {
            (((point[axis] - low[axis]) / span[axis]) * cells[axis] as f32)
                .floor()
                .clamp(0.0, (cells[axis] - 1) as f32) as isize
        })
    };
    let mut grid = std::collections::HashMap::<(isize, isize, isize), Vec<usize>>::new();
    for (index, point) in reference.iter().enumerate() {
        let at = cell(point);
        grid.entry((at[0], at[1], at[2])).or_default().push(index);
    }
    let mut found = Vec::new();
    for (index, point) in moving.iter().enumerate() {
        let moved = [0, 1, 2].map(|axis| point[axis] - shift[axis]);
        let at = cell(&moved);
        let mut best: Option<(f32, usize)> = None;
        for dz in -NEIGHBOURHOOD..=NEIGHBOURHOOD {
            for dy in -NEIGHBOURHOOD..=NEIGHBOURHOOD {
                for dx in -NEIGHBOURHOOD..=NEIGHBOURHOOD {
                    let key = (at[0] + dx, at[1] + dy, at[2] + dz);
                    if key.0 < 0
                        || key.1 < 0
                        || key.2 < 0
                        || key.0 >= cells[0]
                        || key.1 >= cells[1]
                        || key.2 >= cells[2]
                    {
                        continue;
                    }
                    let Some(members) = grid.get(&key) else {
                        continue;
                    };
                    for member in members {
                        let other = &reference[*member];
                        let offset = [0, 1, 2].map(|axis| other[axis] - moved[axis]);
                        let distance = offset.iter().map(|value| value * value).sum::<f32>();
                        if distance <= reach * reach && best.is_none_or(|(best, _)| distance < best)
                        {
                            best = Some((distance, *member));
                        }
                    }
                }
            }
        }
        if let Some((distance, member)) = best {
            found.push((distance, index, member));
        }
    }
    found.sort_by(|left, right| left.0.total_cmp(&right.0));
    let mut claimed = vec![false; reference.len()];
    let mut paired = Vec::with_capacity(found.len());
    for (_, index, member) in found {
        if claimed[member] {
            continue;
        }
        claimed[member] = true;
        paired.push((reference[member], moving[index]));
    }
    paired
}

fn lattice(low: [f32; 3], high: [f32; 3]) -> Lattice {
    let nodes = [0, 1, 2].map(|axis| if high[axis] > low[axis] { NODES } else { 1 });
    Lattice::covering((low, high), nodes)
}

fn trim(samples: &mut Vec<Sample>, lattice: Lattice, scale: f32) -> usize {
    for _ in 0..3 {
        let Some(warp) = drift::deform(samples, lattice, scale, drift::Smoothing::EXACT) else {
            break;
        };
        let mut residuals = samples
            .iter()
            .map(|sample| {
                let offset =
                    [0, 1, 2].map(|axis| sample.shift[axis] - warp.sample(sample.point)[axis]);
                offset.iter().map(|value| value * value).sum::<f32>().sqrt()
            })
            .collect::<Vec<_>>();
        residuals.sort_by(f32::total_cmp);
        let median = residuals[residuals.len() / 2];
        let deviation = residuals
            .iter()
            .map(|value| (value - median).abs())
            .collect::<Vec<_>>();
        let mad = median_of(&deviation);
        let limit = (median + TRIM * mad).max(scale);
        let before = samples.len();
        let kept = samples
            .iter()
            .filter(|sample| {
                let offset =
                    [0, 1, 2].map(|axis| sample.shift[axis] - warp.sample(sample.point)[axis]);
                offset.iter().map(|value| value * value).sum::<f32>() <= limit * limit
            })
            .copied()
            .collect::<Vec<_>>();
        if kept.len() == before || kept.len() < MINIMUM_PAIRS {
            break;
        }
        *samples = kept;
    }
    samples.len()
}

fn median_of(values: &[f32]) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f32::total_cmp);
    sorted[sorted.len() / 2]
}
