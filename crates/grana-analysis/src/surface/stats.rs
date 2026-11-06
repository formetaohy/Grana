use std::ops::Range;

use grana_volume::Intensity;

use super::{Gate, SurfaceBuilder};
use crate::local::{BLOCK, BlockField, block_levels};

const BUCKET: usize = (BLOCK * BLOCK * BLOCK) as usize;

pub(super) struct Levels {
    anchor: [i64; 3],
    region: [Range<u32>; 3],
    cover: [Range<u32>; 3],
    width: usize,
    height: usize,
    blocks: [usize; 3],
    intensity: Intensity,
    workers: usize,
    background: Vec<f32>,
    signal: Vec<f32>,
    buckets: Vec<f32>,
    counts: Vec<u32>,
    levels: Vec<Option<(f32, f32)>>,
    row: i64,
}

impl Levels {
    pub(super) fn new(plan: &SurfaceBuilder, cover: &[Range<u32>; 3], workers: usize) -> Self {
        let blocks = plan.blocks();
        let columns = blocks[0] * blocks[1];
        assert!(
            plan.region.iter().all(|range| !range.is_empty()),
            "a block field covers a non empty region"
        );
        Self {
            anchor: plan.anchor,
            region: plan.region.clone(),
            cover: cover.clone(),
            width: (cover[0].end - cover[0].start) as usize,
            height: (cover[1].end - cover[1].start) as usize,
            blocks,
            intensity: plan.intensity,
            workers: workers.max(1),
            background: vec![f32::NAN; columns * blocks[2]],
            signal: vec![f32::NAN; columns * blocks[2]],
            buckets: vec![0.0; columns * BUCKET],
            counts: vec![0; columns],
            levels: vec![None; columns],
            row: i64::MIN,
        }
    }

    pub(super) fn push(&mut self, gate: &Gate<'_>, start: u32, values: &[f32], planes: usize) {
        let plane = self.width * self.height;
        assert!(
            !values.is_empty() && values.len() == plane * planes,
            "a detection band holds whole planes"
        );
        let end = start.saturating_add(planes as u32);
        let low = start.max(self.region[2].start);
        let high = end.min(self.region[2].end);
        let mut z = low;
        while z < high {
            let offset = i64::from(z) - self.anchor[2];
            let row = offset.div_euclid(BLOCK);
            let row_end = ((i64::from(z) + BLOCK - offset.rem_euclid(BLOCK)) as u32).min(high);
            if row != self.row {
                self.flush();
                self.row = row;
            }
            self.push_rows(gate, start, z, row_end, values);
            z = row_end;
        }
    }

    fn push_rows(&mut self, gate: &Gate<'_>, start: u32, low: u32, high: u32, values: &[f32]) {
        let blocks = self.blocks;
        let region = &self.region;
        let cover = &self.cover;
        let width = self.width;
        let plane = self.width * self.height;
        let anchor = self.anchor;
        let chunk = blocks[1].div_ceil(self.workers).max(1);
        let gated = gate.roi.is_some();
        let counts = &mut self.counts[..];
        let buckets = &mut self.buckets[..];
        crate::schedule::parallel(|scope| {
            for (group, (counts, buckets)) in counts
                .chunks_mut(blocks[0] * chunk)
                .zip(buckets.chunks_mut(blocks[0] * chunk * BUCKET))
                .enumerate()
            {
                let first = group * chunk;
                scope.spawn(move |_| {
                    for (offset, (counts, buckets)) in counts
                        .chunks_mut(blocks[0])
                        .zip(buckets.chunks_mut(blocks[0] * BUCKET))
                        .enumerate()
                    {
                        let by = first + offset;
                        if by >= blocks[1] {
                            break;
                        }
                        let first_y = anchor[1] + (by * BLOCK as usize) as i64;
                        let y0 = first_y
                            .max(i64::from(region[1].start))
                            .max(i64::from(cover[1].start));
                        let y1 = (first_y + BLOCK)
                            .min(i64::from(region[1].end))
                            .min(i64::from(cover[1].end));
                        if y1 <= y0 {
                            continue;
                        }
                        let mut spans = Vec::with_capacity(blocks[0]);
                        for bx in 0..blocks[0] {
                            let first_x = anchor[0] + (bx * BLOCK as usize) as i64;
                            let x0 = first_x
                                .max(i64::from(region[0].start))
                                .max(i64::from(cover[0].start));
                            let x1 = (first_x + BLOCK)
                                .min(i64::from(region[0].end))
                                .min(i64::from(cover[0].end));
                            spans.push((x0, x1));
                        }
                        for z in low..high {
                            let base = (z - start) as usize * plane;
                            for (bx, (count, bucket)) in counts
                                .iter_mut()
                                .zip(buckets.chunks_mut(BUCKET))
                                .enumerate()
                            {
                                let (x0, x1) = spans[bx];
                                if x1 <= x0 {
                                    continue;
                                }
                                let mut stored = *count as usize;
                                for y in y0..y1 {
                                    let origin = (y - i64::from(cover[1].start)) as usize * width;
                                    for x in x0..x1 {
                                        if gated && !gate.admits([x, y, i64::from(z)]) {
                                            continue;
                                        }
                                        let slot =
                                            origin + (x - i64::from(cover[0].start)) as usize;
                                        bucket[stored] = values[base + slot];
                                        stored += 1;
                                    }
                                }
                                assert!(
                                    stored <= BUCKET,
                                    "an eight voxel block fills its bucket once"
                                );
                                *count = stored as u32;
                            }
                        }
                    }
                });
            }
        });
    }

    pub(super) fn finish(mut self) -> BlockField {
        self.flush();
        BlockField::from_blocks(
            self.anchor,
            self.blocks,
            self.intensity,
            self.background,
            self.signal,
            self.workers,
        )
    }

    fn flush(&mut self) {
        if self.row == i64::MIN {
            return;
        }
        let blocks = self.blocks;
        let columns = blocks[0] * blocks[1];
        let chunk = columns.div_ceil(self.workers).max(1);
        {
            let buckets = &mut self.buckets[..];
            let counts = &self.counts[..];
            let levels = &mut self.levels[..];
            crate::schedule::parallel(|scope| {
                for ((bucket, counts), levels) in buckets
                    .chunks_mut(chunk * BUCKET)
                    .zip(counts.chunks(chunk))
                    .zip(levels.chunks_mut(chunk))
                {
                    scope.spawn(move |_| {
                        for (bucket, (count, level)) in bucket
                            .chunks_mut(BUCKET)
                            .zip(counts.iter().zip(levels.iter_mut()))
                        {
                            *level = block_levels(&mut bucket[..*count as usize]);
                        }
                    });
                }
            });
        }
        let z = self.row as usize;
        for (cell, level) in self.levels.iter().enumerate() {
            let Some((background, signal)) = level else {
                continue;
            };
            let bx = cell % blocks[0];
            let by = cell / blocks[0];
            let slot = bx + blocks[0] * (by + blocks[1] * z);
            self.background[slot] = *background;
            self.signal[slot] = *signal;
        }
        self.levels.fill(None);
        self.counts.fill(0);
    }
}
