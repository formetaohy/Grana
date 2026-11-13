use std::collections::HashSet;
use std::ops::Range;
use std::sync::atomic::{AtomicBool, Ordering};

use grana_object::CellParams;
use grana_volume::Level;

use super::ridge::{self, BINS};
use super::{CellField, CellSeed, SNAP, dims};

pub(super) struct Seeds<'a> {
    pub level: Level,
    pub ranges: &'a [Range<u32>; 3],
    pub params: CellParams,
    pub seeds: &'a [CellSeed],
    pub source: &'a dyn CellField,
    pub workers: usize,
    pub cancel: &'a AtomicBool,
}

pub(super) fn barrier(context: &Seeds<'_>) -> Result<Option<u16>, String> {
    let Seeds {
        level,
        ranges,
        params,
        seeds,
        source,
        workers,
        cancel,
    } = context;
    let steps = level.voxel();
    let halo = ridge::halo(*level, params.grain()) as i64;
    let mut histogram = [0u64; BINS];
    for seed in *seeds {
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let inner = seed.radius();
        let outer = params.reach() * seed.radius();
        if outer <= inner {
            continue;
        }
        let centre = level.voxel_of(seed.centre());
        let reach = [0, 1, 2].map(|axis| ((outer / steps[axis]).ceil() as i64) + halo);
        let Some(shell) = clipped(centre, reach, ranges) else {
            continue;
        };
        let values = source.values(&shell)?;
        let shell_dims = dims(&shell);
        assert_eq!(
            values.len(),
            shell_dims[0] * shell_dims[1] * shell_dims[2],
            "the membrane field covers the shell of a seed"
        );
        let Some(field) = ridge::elevations(
            &values,
            shell_dims,
            *level,
            params.grain(),
            *workers,
            cancel,
        ) else {
            return Ok(None);
        };
        ridge::sample(&mut histogram, &field, &shell, centre, inner, outer, steps);
    }
    Ok(Some(ridge::threshold(&histogram)))
}

pub(super) fn snap(
    context: &Seeds<'_>,
    barrier: u16,
) -> Result<Option<Vec<Option<[f32; 3]>>>, String> {
    let Seeds {
        level,
        ranges,
        params,
        seeds,
        source,
        workers,
        cancel,
    } = context;
    let halo = ridge::halo(*level, params.grain()) as i64 + SNAP;
    let mut taken = HashSet::new();
    let mut snapped = Vec::with_capacity(seeds.len());
    for seed in *seeds {
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let centre = level.voxel_of(seed.centre());
        let Some(snap_box) = clipped(centre, [halo; 3], ranges) else {
            snapped.push(None);
            continue;
        };
        let values = source.values(&snap_box)?;
        let box_dims = dims(&snap_box);
        assert_eq!(
            values.len(),
            box_dims[0] * box_dims[1] * box_dims[2],
            "the membrane field covers the snap box of a seed"
        );
        let Some(field) =
            ridge::elevations(&values, box_dims, *level, params.grain(), *workers, cancel)
        else {
            return Ok(None);
        };
        let base = centre.map(|value| value.floor() as i64);
        let mut best: Option<(u16, i64, [u32; 3])> = None;
        for dz in -SNAP..=SNAP {
            for dy in -SNAP..=SNAP {
                for dx in -SNAP..=SNAP {
                    let at = [base[0] + dx, base[1] + dy, base[2] + dz];
                    let local = [0, 1, 2].map(|axis| at[axis] - snap_box[axis].start as i64);
                    if (0..3).any(|axis| local[axis] < 0 || local[axis] >= box_dims[axis] as i64) {
                        continue;
                    }
                    let voxel = at.map(|value| value as u32);
                    if taken.contains(&voxel) {
                        continue;
                    }
                    let index = local[0] as usize
                        + box_dims[0] * (local[1] as usize + box_dims[1] * local[2] as usize);
                    let altitude = field[index];
                    if altitude > barrier {
                        continue;
                    }
                    let candidate = (altitude, dx * dx + dy * dy + dz * dz, voxel);
                    if best.is_none_or(|best| (candidate.0, candidate.1) < (best.0, best.1)) {
                        best = Some(candidate);
                    }
                }
            }
        }
        match best {
            Some((_, _, voxel)) => {
                taken.insert(voxel);
                snapped.push(Some(voxel.map(|value| value as f32)));
            }
            None => snapped.push(None),
        }
    }
    Ok(Some(snapped))
}

fn clipped(centre: [f32; 3], pad: [i64; 3], ranges: &[Range<u32>; 3]) -> Option<[Range<u32>; 3]> {
    let mut boxed = [0u32..0u32, 0u32..0u32, 0u32..0u32];
    for axis in 0..3 {
        let base = centre[axis].floor() as i64;
        let start = (base - pad[axis]).max(ranges[axis].start as i64);
        let end = (base + pad[axis] + 1).min(ranges[axis].end as i64);
        if start >= end {
            return None;
        }
        boxed[axis] = start as u32..end as u32;
    }
    Some(boxed)
}
