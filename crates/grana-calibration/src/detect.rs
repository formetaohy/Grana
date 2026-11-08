use std::cmp::Ordering;

use grana_image::Image;
use grana_volume::Spacing;
use rayon::prelude::*;

use crate::error::{Error, calibration};

const SAMPLE_LIMIT: usize = 2 << 20;
const CANDIDATE_LIMIT: usize = 50_000;
const NOISE_FLOOR: f32 = 1.0e-6;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Bead {
    pub(crate) world: [f32; 3],
}

pub(crate) fn detection_level(image: &Image, budget: u64) -> usize {
    let stored = image.pyramid().stored();
    let mut level = 0;
    while level + 1 < stored && image.pyramid().level(level).extent().voxel_count() > budget {
        level += 1;
    }
    level
}

pub(crate) fn read_level(
    image: &Image,
    time: u32,
    channel: usize,
    level: usize,
) -> Result<Vec<f32>, Error> {
    let extent = image.pyramid().level(level).extent();
    let ranges = [0..extent.x(), 0..extent.y(), 0..extent.z()];
    let data = image.read_region(time, level, channel, &ranges, [0.0; 3])?;
    let mut values = vec![0.0f32; data.voxel_count()];
    data.write_values(&mut values);
    Ok(values)
}

pub(crate) fn background(values: &[f32]) -> Result<(f32, f32), Error> {
    let stride = (values.len() / SAMPLE_LIMIT).max(1);
    let mut sample = values
        .iter()
        .step_by(stride)
        .copied()
        .filter(|value| value.is_finite())
        .collect::<Vec<_>>();
    if sample.is_empty() {
        return Err(calibration("the calibration channel holds no finite voxel"));
    }
    sample.sort_unstable_by(f32::total_cmp);
    let median = sample[sample.len() / 2];
    let mut deviations = sample
        .iter()
        .map(|value| (value - median).abs())
        .collect::<Vec<_>>();
    deviations.sort_unstable_by(f32::total_cmp);
    let mad = deviations[deviations.len() / 2];
    let sigma = (1.4826 * mad).max(NOISE_FLOOR);
    Ok((median, sigma))
}

pub(crate) fn beads(
    values: &[f32],
    dims: [u32; 3],
    origin: [f32; 3],
    spacing: Spacing,
    threshold: f32,
    separation: f32,
    limit: usize,
) -> Result<Vec<Bead>, Error> {
    let (median, sigma) = background(values)?;
    let floor = median + threshold * sigma;
    let width = dims[0] as usize;
    let height = dims[1] as usize;
    let plane = width * height;
    let mut candidates = (1..dims[2].saturating_sub(1))
        .into_par_iter()
        .flat_map_iter(|z| {
            let mut plane_candidates = Vec::new();
            for y in 1..dims[1].saturating_sub(1) {
                for x in 1..dims[0].saturating_sub(1) {
                    let index = x as usize + width * (y as usize + height * z as usize);
                    let value = values[index];
                    if !value.is_finite() || value <= floor {
                        continue;
                    }
                    if is_maximum(values, index, width, plane, value) {
                        plane_candidates.push((value, [x, y, z]));
                    }
                }
            }
            plane_candidates
        })
        .collect::<Vec<_>>();
    candidates.sort_unstable_by(|left, right| {
        right
            .0
            .partial_cmp(&left.0)
            .unwrap_or(Ordering::Equal)
            .then_with(|| left.1.cmp(&right.1))
    });
    candidates.truncate(CANDIDATE_LIMIT);
    let mut beads: Vec<Bead> = Vec::new();
    let separation = separation.max(spacing.x().max(spacing.y()).max(spacing.z()));
    for (_, coord) in candidates {
        let world = [
            origin[0] + (coord[0] as f32 + 0.5) * spacing.x(),
            origin[1] + (coord[1] as f32 + 0.5) * spacing.y(),
            origin[2] + (coord[2] as f32 + 0.5) * spacing.z(),
        ];
        if beads
            .iter()
            .any(|bead| distance(bead.world, world) < separation)
        {
            continue;
        }
        beads.push(Bead { world });
        if beads.len() >= limit {
            break;
        }
    }
    if beads.is_empty() {
        return Err(calibration(
            "no isolated bead rises above the background of the selected channel",
        ));
    }
    Ok(beads)
}

fn is_maximum(values: &[f32], index: usize, width: usize, plane: usize, value: f32) -> bool {
    for z in -1isize..=1 {
        for y in -1isize..=1 {
            for x in -1isize..=1 {
                if x == 0 && y == 0 && z == 0 {
                    continue;
                }
                let neighbour =
                    (index as isize + x + width as isize * y + plane as isize * z) as usize;
                let sample = values[neighbour];
                if sample > value {
                    return false;
                }
                if sample == value && neighbour < index {
                    return false;
                }
            }
        }
    }
    true
}

fn distance(left: [f32; 3], right: [f32; 3]) -> f32 {
    (0..3)
        .map(|axis| (left[axis] - right[axis]).powi(2))
        .sum::<f32>()
        .sqrt()
}
