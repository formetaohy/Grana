use std::sync::atomic::AtomicBool;

use super::{Gate, Read, SurfaceBuilder, dims_of, smooth};
use crate::local::{BLOCK, BlockField, block_levels};

const SLAB_BLOCKS: usize = 8;

pub(super) struct Slabs {
    blocks: [usize; 3],
    counts: [usize; 3],
}

impl Slabs {
    pub(super) fn new(blocks: [usize; 3]) -> Self {
        let counts = blocks.map(|value| value.div_ceil(SLAB_BLOCKS));
        Self { blocks, counts }
    }

    pub(super) fn total(&self) -> usize {
        self.counts[0] * self.counts[1] * self.counts[2]
    }

    fn origin(&self, index: usize) -> [usize; 3] {
        [
            (index % self.counts[0]) * SLAB_BLOCKS,
            ((index / self.counts[0]) % self.counts[1]) * SLAB_BLOCKS,
            (index / (self.counts[0] * self.counts[1])) * SLAB_BLOCKS,
        ]
    }

    fn dims(&self, index: usize) -> [usize; 3] {
        let origin = self.origin(index);
        [0, 1, 2].map(|axis| (self.blocks[axis] - origin[axis]).min(SLAB_BLOCKS))
    }
}

pub(super) fn build(
    plan: &SurfaceBuilder,
    workers: usize,
    cancel: &AtomicBool,
    gate: &Gate<'_>,
    read: &Read<'_>,
    progress: &mut dyn FnMut(usize, usize),
) -> Result<Option<BlockField>, String> {
    let blocks = plan.blocks();
    let slabs = Slabs::new(blocks);
    let total = slabs.total();
    let count = blocks[0] * blocks[1] * blocks[2];
    let mut background = vec![f32::NAN; count];
    let mut signal = vec![f32::NAN; count];
    let stride = [1, blocks[0], blocks[0] * blocks[1]];
    let completed = crate::schedule::ordered(
        crate::schedule::Run {
            total,
            workers,
            cancel,
            progress,
        },
        Vec::new,
        |buffer, index| collect(plan, gate, &slabs, index, read, buffer),
        |index, levels: Vec<(f32, f32)>| {
            let origin = slabs.origin(index);
            let dims = slabs.dims(index);
            let mut slot = 0usize;
            for z in 0..dims[2] {
                for y in 0..dims[1] {
                    for x in 0..dims[0] {
                        let target = stride[0] * (origin[0] + x)
                            + stride[1] * (origin[1] + y)
                            + stride[2] * (origin[2] + z);
                        (background[target], signal[target]) = levels[slot];
                        slot += 1;
                    }
                }
            }
            Ok(())
        },
        |_| {},
    )?;
    if !completed {
        return Ok(None);
    }
    Ok(Some(BlockField::from_blocks(
        plan.anchor,
        blocks,
        plan.intensity,
        background,
        signal,
    )))
}

fn collect(
    plan: &SurfaceBuilder,
    gate: &Gate<'_>,
    slabs: &Slabs,
    index: usize,
    read: &Read<'_>,
    buffer: &mut Vec<f32>,
) -> Result<Vec<(f32, f32)>, String> {
    let origin = slabs.origin(index);
    let dims = slabs.dims(index);
    let radius = [0, 1, 2].map(|axis| (plan.kernels[axis].len() / 2) as u32);
    let span = [0, 1, 2].map(|axis| {
        let first = plan.anchor[axis] + (origin[axis] * BLOCK as usize) as i64;
        let last =
            (first + (dims[axis] * BLOCK as usize) as i64).min(i64::from(plan.region[axis].end));
        first..last
    });
    let ranges = [0, 1, 2].map(|axis| {
        let low = (span[axis].start - i64::from(radius[axis])).max(0);
        let high = (span[axis].end + i64::from(radius[axis])).min(i64::from(plan.extent[axis]));
        low as u32..high as u32
    });
    let array = dims_of(&ranges);
    let count = array[0] * array[1] * array[2];
    let data = read(index, &ranges)?;
    if data.voxel_count() != count {
        return Err("the detection slab does not cover the requested region".to_string());
    }
    buffer.clear();
    buffer.resize(count, 0.0);
    data.write_values(buffer);
    smooth(buffer, array, &plan.kernels);
    let offset = [
        (span[0].start - i64::from(ranges[0].start)) as usize,
        (span[1].start - i64::from(ranges[1].start)) as usize,
        (span[2].start - i64::from(ranges[2].start)) as usize,
    ];
    let lines = [1usize, array[0], array[0] * array[1]];
    let mut levels = vec![(f32::NAN, f32::NAN); dims[0] * dims[1] * dims[2]];
    let mut bucket = Vec::with_capacity((BLOCK * BLOCK * BLOCK) as usize);
    for z in 0..dims[2] {
        for y in 0..dims[1] {
            for x in 0..dims[0] {
                bucket.clear();
                for local_z in 0..BLOCK as usize {
                    let voxel_z = span[2].start + ((z * BLOCK as usize) + local_z) as i64;
                    if voxel_z >= span[2].end {
                        break;
                    }
                    let row_z = offset[2] + z * BLOCK as usize + local_z;
                    for local_y in 0..BLOCK as usize {
                        let voxel_y = span[1].start + ((y * BLOCK as usize) + local_y) as i64;
                        if voxel_y >= span[1].end {
                            break;
                        }
                        let row_y = offset[1] + y * BLOCK as usize + local_y;
                        let base =
                            lines[1] * row_y + lines[2] * row_z + offset[0] + x * BLOCK as usize;
                        for local_x in 0..BLOCK as usize {
                            let voxel_x = span[0].start + ((x * BLOCK as usize) + local_x) as i64;
                            if voxel_x >= span[0].end {
                                break;
                            }
                            if !gate.admits([voxel_x, voxel_y, voxel_z]) {
                                continue;
                            }
                            bucket.push(buffer[base + local_x]);
                        }
                    }
                }
                if let Some(level) = block_levels(&mut bucket) {
                    levels[x + dims[0] * (y + dims[1] * z)] = level;
                }
            }
        }
    }
    Ok(levels)
}
