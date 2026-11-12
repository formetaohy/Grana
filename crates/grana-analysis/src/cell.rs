use std::ops::Range;
use std::sync::atomic::{AtomicBool, Ordering};

use grana_object::{CellParams, Mask};
use grana_volume::Level;

use crate::schedule;

mod ridge;

const LEVELS: usize = 1 << 16;
const TOP: u16 = (LEVELS - 1) as u16;
const CHECK: usize = 1 << 16;
const SEED_SNAP: i64 = 2;

pub struct CellSeed {
    centre: [f32; 3],
    radius: f32,
}

impl CellSeed {
    pub fn new(centre: [f32; 3], radius: f32) -> Self {
        assert!(
            centre.iter().all(|value| value.is_finite()),
            "a cell seed names a finite centre"
        );
        assert!(
            radius.is_finite() && radius > 0.0,
            "a cell seed names a positive nucleus radius"
        );
        Self { centre, radius }
    }
}

pub struct GrowField<'a> {
    pub level: Level,
    pub ranges: [Range<u32>; 3],
    pub params: CellParams,
    pub values: &'a [f32],
    pub mask: Option<&'a Mask>,
    pub seeds: &'a [CellSeed],
}

pub struct Growth {
    ranges: [Range<u32>; 3],
    labels: Vec<u32>,
    nuclei: Vec<usize>,
}

impl Growth {
    pub fn ranges(&self) -> &[Range<u32>; 3] {
        &self.ranges
    }

    pub fn labels(&self) -> &[u32] {
        &self.labels
    }

    pub fn nuclei(&self) -> &[usize] {
        &self.nuclei
    }
}

pub fn grow(field: GrowField<'_>, cancel: &AtomicBool) -> Option<Growth> {
    let GrowField {
        level,
        ranges,
        params,
        values,
        mask,
        seeds,
    } = field;
    let dims = dims(&ranges);
    let count = dims[0] * dims[1] * dims[2];
    if count == 0 || seeds.is_empty() {
        return None;
    }
    let workers = schedule::workers(0);
    let elevations = ridge::elevations(values, dims, level, params.grain(), workers, cancel)?;
    if cancel.load(Ordering::Relaxed) {
        return None;
    }
    let centres = seeds
        .iter()
        .map(|seed| {
            let voxel = level.voxel_of(seed.centre);
            [
                voxel[0] - ranges[0].start as f32,
                voxel[1] - ranges[1].start as f32,
                voxel[2] - ranges[2].start as f32,
            ]
        })
        .collect::<Vec<_>>();
    let shells = seeds
        .iter()
        .zip(&centres)
        .map(|(seed, centre)| ridge::Shell {
            centre: *centre,
            inner: seed.radius,
            outer: params.reach() * seed.radius,
        })
        .collect::<Vec<_>>();
    let barrier = ridge::barrier(&elevations, dims, level.voxel(), &shells);

    let steps = level.voxel().map(|step| step * step);
    let reaches = seeds
        .iter()
        .map(|seed| (params.reach() * seed.radius).powi(2))
        .collect::<Vec<_>>();
    let mut labels = vec![0u32; count];
    let mut queue = Queue::new();
    for (index, centre) in centres.iter().enumerate() {
        let Some(voxel) = snap(centre, dims, &labels, &elevations, barrier) else {
            continue;
        };
        labels[offset(&voxel, &dims)] = index as u32 + 1;
        push(
            &mut queue,
            &elevations,
            &labels,
            &voxel,
            &dims,
            barrier,
            mask,
            &ranges,
        );
    }
    if labels.iter().all(|label| *label == 0) {
        return None;
    }
    let mut visited = 0usize;
    while let Some(at) = queue.pop(barrier as usize) {
        visited += 1;
        if visited.is_multiple_of(CHECK) && cancel.load(Ordering::Relaxed) {
            return None;
        }
        if labels[at] != 0 {
            continue;
        }
        let Some(owner) = owner(&labels, &elevations, &dims, at) else {
            continue;
        };
        let voxel = voxel(at, &dims);
        let centre = &centres[owner as usize - 1];
        let delta = [0, 1, 2].map(|axis| (voxel[axis] as f32 + 0.5 - centre[axis]).powi(2));
        let distance = delta[0] * steps[0] + delta[1] * steps[1] + delta[2] * steps[2];
        if distance > reaches[owner as usize - 1] {
            continue;
        }
        labels[at] = owner;
        push(
            &mut queue,
            &elevations,
            &labels,
            &voxel,
            &dims,
            barrier,
            mask,
            &ranges,
        );
    }
    if cancel.load(Ordering::Relaxed) {
        return None;
    }
    compact(labels, seeds.len()).map(|(labels, nuclei)| Growth {
        ranges,
        labels,
        nuclei,
    })
}

fn compact(labels: Vec<u32>, seeds: usize) -> Option<(Vec<u32>, Vec<usize>)> {
    let mut present = vec![false; seeds];
    for label in &labels {
        if *label != 0 {
            present[*label as usize - 1] = true;
        }
    }
    let mut slots = vec![0u32; seeds + 1];
    let mut nuclei = Vec::new();
    for (index, held) in present.iter().enumerate() {
        if *held {
            nuclei.push(index);
            slots[index + 1] = nuclei.len() as u32;
        }
    }
    if nuclei.is_empty() {
        return None;
    }
    let mut compacted = labels;
    for label in &mut compacted {
        *label = slots[*label as usize];
    }
    Some((compacted, nuclei))
}

#[allow(clippy::too_many_arguments)]
fn push(
    queue: &mut Queue,
    elevations: &[u16],
    labels: &[u32],
    voxel: &[usize; 3],
    dims: &[usize; 3],
    barrier: u16,
    mask: Option<&Mask>,
    ranges: &[Range<u32>; 3],
) {
    for axis in 0..3 {
        for direction in [-1i64, 1] {
            let target = voxel[axis] as i64 + direction;
            if target < 0 || target >= dims[axis] as i64 {
                continue;
            }
            let mut next = *voxel;
            next[axis] = target as usize;
            let neighbour = offset(&next, dims);
            if labels[neighbour] != 0 {
                continue;
            }
            let altitude = elevations[neighbour];
            if altitude > barrier {
                continue;
            }
            if let Some(mask) = mask {
                let at = [
                    ranges[0].start + next[0] as u32,
                    ranges[1].start + next[1] as u32,
                    ranges[2].start + next[2] as u32,
                ];
                if !mask.contains_cell(at) {
                    continue;
                }
            }
            queue.push(altitude as usize, neighbour);
        }
    }
}

fn owner(labels: &[u32], elevations: &[u16], dims: &[usize; 3], at: usize) -> Option<u32> {
    let voxel = voxel(at, dims);
    let mut best: Option<(u16, u32)> = None;
    for axis in 0..3 {
        for direction in [-1i64, 1] {
            let target = voxel[axis] as i64 + direction;
            if target < 0 || target >= dims[axis] as i64 {
                continue;
            }
            let mut next = voxel;
            next[axis] = target as usize;
            let neighbour = offset(&next, dims);
            let label = labels[neighbour];
            if label == 0 {
                continue;
            }
            let candidate = (elevations[neighbour], label);
            if best.is_none_or(|best| candidate < best) {
                best = Some(candidate);
            }
        }
    }
    best.map(|(_, label)| label)
}

fn snap(
    centre: &[f32; 3],
    dims: [usize; 3],
    labels: &[u32],
    elevations: &[u16],
    barrier: u16,
) -> Option<[usize; 3]> {
    let base = [0, 1, 2].map(|axis| centre[axis].floor() as i64);
    let mut best: Option<(u16, i64, [usize; 3])> = None;
    for dz in -SEED_SNAP..=SEED_SNAP {
        for dy in -SEED_SNAP..=SEED_SNAP {
            for dx in -SEED_SNAP..=SEED_SNAP {
                let at = [base[0] + dx, base[1] + dy, base[2] + dz];
                if (0..3).any(|axis| at[axis] < 0 || at[axis] >= dims[axis] as i64) {
                    continue;
                }
                let voxel = at.map(|value| value as usize);
                let index = offset(&voxel, &dims);
                if labels[index] != 0 {
                    continue;
                }
                let altitude = elevations[index];
                if altitude > barrier {
                    continue;
                }
                let distance = dx * dx + dy * dy + dz * dz;
                let candidate = (altitude, distance, voxel);
                if best.is_none_or(|best| (candidate.0, candidate.1) < (best.0, best.1)) {
                    best = Some(candidate);
                }
            }
        }
    }
    best.map(|(_, _, voxel)| voxel)
}

pub(crate) fn elevation(value: f32) -> u16 {
    if !value.is_finite() || value <= 0.0 {
        return 0;
    }
    if value >= 1.0 {
        return TOP;
    }
    (value * f32::from(TOP)).round() as u16
}

fn dims(ranges: &[Range<u32>; 3]) -> [usize; 3] {
    [
        (ranges[0].end - ranges[0].start) as usize,
        (ranges[1].end - ranges[1].start) as usize,
        (ranges[2].end - ranges[2].start) as usize,
    ]
}

fn offset(voxel: &[usize; 3], dims: &[usize; 3]) -> usize {
    voxel[0] + dims[0] * (voxel[1] + dims[1] * voxel[2])
}

fn voxel(offset: usize, dims: &[usize; 3]) -> [usize; 3] {
    [
        offset % dims[0],
        (offset / dims[0]) % dims[1],
        offset / (dims[0] * dims[1]),
    ]
}

struct Queue {
    buckets: Vec<Vec<u32>>,
    cursor: usize,
}

impl Queue {
    fn new() -> Self {
        Self {
            buckets: vec![Vec::new(); LEVELS],
            cursor: 0,
        }
    }

    fn push(&mut self, level: usize, offset: usize) {
        let level = level.max(self.cursor);
        self.buckets[level].push(offset as u32);
    }

    fn pop(&mut self, limit: usize) -> Option<usize> {
        while self.cursor <= limit {
            if let Some(offset) = self.buckets[self.cursor].pop() {
                return Some(offset as usize);
            }
            self.cursor += 1;
            if self.cursor >= LEVELS {
                return None;
            }
        }
        None
    }
}
