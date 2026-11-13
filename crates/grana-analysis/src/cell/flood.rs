use std::sync::atomic::{AtomicBool, Ordering};

use grana_object::Mask;

use super::{CHECK, LEVELS};

pub(crate) struct Sown {
    slot: usize,
    voxel: [usize; 3],
    centre: [f32; 3],
    squared: f32,
}

impl Sown {
    pub(crate) fn new(slot: usize, voxel: [usize; 3], centre: [f32; 3], reach: f32) -> Self {
        assert!(
            reach.is_finite() && reach > 0.0,
            "a sown seed reaches a positive distance"
        );
        Self {
            slot,
            voxel,
            centre,
            squared: reach * reach,
        }
    }
}

pub(crate) struct Flood<'a> {
    pub labels: &'a mut [u32],
    pub elevations: &'a [u16],
    pub dims: [usize; 3],
    pub origin: [u32; 3],
    pub steps: [f32; 3],
    pub barrier: u16,
    pub mask: Option<&'a Mask>,
    pub sown: &'a [Sown],
    pub slots: &'a [u32],
}

impl Flood<'_> {
    pub(crate) fn run(mut self, cancel: &AtomicBool) -> bool {
        let voxels = self.dims[0] * self.dims[1] * self.dims[2];
        assert_eq!(
            self.labels.len(),
            voxels,
            "the labels cover every voxel of the band"
        );
        assert_eq!(
            self.elevations.len(),
            voxels,
            "the elevations cover every voxel of the band"
        );
        let squared = self.steps.map(|step| step * step);
        let mut queue = Queue::new();
        for seed in self.sown {
            self.labels[offset(&seed.voxel, &self.dims)] = seed.slot as u32 + 1;
            self.push(&mut queue, &seed.voxel);
        }
        let mut visited = 0usize;
        while let Some(at) = queue.pop(self.barrier as usize) {
            visited += 1;
            if visited.is_multiple_of(CHECK) && cancel.load(Ordering::Relaxed) {
                return false;
            }
            if self.labels[at] != 0 {
                continue;
            }
            let Some(owner) = self.owner(at) else {
                continue;
            };
            let cell = voxel(at, &self.dims);
            let seed = &self.sown[self.slots[owner as usize] as usize - 1];
            let delta = [0, 1, 2].map(|axis| {
                (self.origin[axis] as f32 + cell[axis] as f32 + 0.5 - seed.centre[axis]).powi(2)
            });
            let distance = delta[0] * squared[0] + delta[1] * squared[1] + delta[2] * squared[2];
            if distance > seed.squared {
                continue;
            }
            self.labels[at] = owner;
            self.push(&mut queue, &cell);
        }
        true
    }

    fn push(&mut self, queue: &mut Queue, cell: &[usize; 3]) {
        for axis in 0..3 {
            for direction in [-1i64, 1] {
                let target = cell[axis] as i64 + direction;
                if target < 0 || target >= self.dims[axis] as i64 {
                    continue;
                }
                let mut next = *cell;
                next[axis] = target as usize;
                let neighbour = offset(&next, &self.dims);
                if self.labels[neighbour] != 0 {
                    continue;
                }
                let altitude = self.elevations[neighbour];
                if altitude > self.barrier {
                    continue;
                }
                if let Some(mask) = self.mask {
                    let at = [
                        self.origin[0] + next[0] as u32,
                        self.origin[1] + next[1] as u32,
                        self.origin[2] + next[2] as u32,
                    ];
                    if !mask.contains_cell(at) {
                        continue;
                    }
                }
                queue.push(altitude as usize, neighbour);
            }
        }
    }

    fn owner(&self, at: usize) -> Option<u32> {
        let cell = voxel(at, &self.dims);
        let mut best: Option<(u16, u32)> = None;
        for axis in 0..3 {
            for direction in [-1i64, 1] {
                let target = cell[axis] as i64 + direction;
                if target < 0 || target >= self.dims[axis] as i64 {
                    continue;
                }
                let mut next = cell;
                next[axis] = target as usize;
                let neighbour = offset(&next, &self.dims);
                let label = self.labels[neighbour];
                if label == 0 {
                    continue;
                }
                let candidate = (self.elevations[neighbour], label);
                if best.is_none_or(|best| candidate < best) {
                    best = Some(candidate);
                }
            }
        }
        best.map(|(_, label)| label)
    }
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
