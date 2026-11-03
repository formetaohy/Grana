use std::sync::atomic::AtomicBool;

use crate::local::{BLOCK, BlockField, LocalField, floor};
use crate::mask::Mask;
use crate::morphology;

use super::bits::{Bits, Grid, MaskInputs, SIDE};
use super::{Gate, Read, SurfaceBuilder, dims_of, parallel_tiles, smooth};

const RAISED_TOLERANCE: f32 = 0.5;

pub(super) struct SurfaceMask {
    pub(super) solid: SolidMask,
    pub(super) field: BlockField,
}

pub(super) enum SolidMask {
    Flooded(Box<Flooded>),
    Materialized(Bits),
}

pub(super) struct Flooded {
    foreground: Bits,
    admitted: Option<Bits>,
    grid: Grid,
}

impl SolidMask {
    pub(super) fn get(&self, voxel: [i64; 3], anchor: [i64; 3]) -> bool {
        let local = [0, 1, 2].map(|axis| voxel[axis] - anchor[axis]);
        if local.iter().any(|value| *value < 0) {
            return false;
        }
        let local = [0, 1, 2].map(|axis| local[axis] as usize);
        match self {
            Self::Flooded(flooded) => flooded.grid.is_solid(
                &flooded.foreground,
                flooded.admitted.as_ref(),
                local[0],
                local[1],
                local[2],
            ),
            Self::Materialized(bits) => bits.get(local[0], local[1], local[2]),
        }
    }
}

pub(super) fn build(
    plan: &SurfaceBuilder,
    workers: usize,
    cancel: &AtomicBool,
    roi: Option<&Mask>,
    progress: &mut dyn FnMut(usize, usize),
    read: &Read<'_>,
) -> Result<Option<SurfaceMask>, String> {
    let dims = plan.dims();
    let blocks = plan.blocks();
    let total = plan.tile_count();
    let step = total * 2;
    let mut foreground = Bits::new(dims);
    let mut raised = Bits::new(dims);
    let mut admitted = roi.map(|_| Bits::new(dims));
    let mut field = BlockField::new(plan.anchor, blocks, floor(plan.intensity));
    let mut occupied = vec![false; blocks[0] * blocks[1] * blocks[2]];
    let gate = plan.gate(roi);
    let pad = plan.pad_mask();
    let completed = parallel_tiles(
        total,
        workers,
        cancel,
        &mut |done, _| progress(done, step),
        |index| MaskWorker::run(plan, &gate, index, pad, read),
        |index, chunk| {
            let mut sink = MaskSink {
                foreground: &mut foreground,
                raised: &mut raised,
                admitted: admitted.as_mut(),
                field: &mut field,
                occupied: &mut occupied,
            };
            write_chunk(plan, index, chunk, &mut sink)
        },
    )?;
    if !completed {
        return Ok(None);
    }
    field.extrapolate();
    let grid = Grid::new(
        &MaskInputs {
            foreground: &foreground,
            admitted: admitted.as_ref(),
            raised: &raised,
            field: &field,
        },
        &occupied,
        blocks,
        dims,
    );
    let solid = if plan.opening.is_some() || plan.closing.is_some() {
        let Some(bits) = morph(
            plan,
            workers,
            cancel,
            progress,
            &foreground,
            admitted.as_ref(),
            &grid,
        )?
        else {
            return Ok(None);
        };
        SolidMask::Materialized(bits)
    } else {
        SolidMask::Flooded(Box::new(Flooded {
            foreground,
            admitted,
            grid,
        }))
    };
    Ok(Some(SurfaceMask { solid, field }))
}

struct MaskSink<'a> {
    foreground: &'a mut Bits,
    raised: &'a mut Bits,
    admitted: Option<&'a mut Bits>,
    field: &'a mut BlockField,
    occupied: &'a mut [bool],
}

fn write_chunk(
    plan: &SurfaceBuilder,
    index: usize,
    chunk: MaskChunk,
    sink: &mut MaskSink<'_>,
) -> Result<(), String> {
    let core = plan.core_range(index);
    let dims = dims_of(&core);
    let origin = [
        (core[0].start - plan.region[0].start) as usize,
        (core[1].start - plan.region[1].start) as usize,
        (core[2].start - plan.region[2].start) as usize,
    ];
    if chunk.foreground.len() != chunk.rows * dims[1] * dims[2] {
        return Err("a mask tile carries the wrong number of words".to_string());
    }
    for z in 0..dims[2] {
        for y in 0..dims[1] {
            let start = chunk.rows * (y + dims[1] * z);
            let row = &chunk.foreground[start..start + chunk.rows];
            sink.foreground
                .write_row(origin[0], origin[1] + y, origin[2] + z, row, dims[0]);
            let row = &chunk.raised[start..start + chunk.rows];
            sink.raised
                .write_row(origin[0], origin[1] + y, origin[2] + z, row, dims[0]);
            if let (Some(bits), Some(values)) =
                (sink.admitted.as_deref_mut(), chunk.admitted.as_ref())
            {
                let row = &values[start..start + chunk.rows];
                bits.write_row(origin[0], origin[1] + y, origin[2] + z, row, dims[0]);
            }
        }
    }
    let blocks = chunk.block_dims;
    let global = plan.blocks();
    for z in 0..blocks[2] {
        for y in 0..blocks[1] {
            for x in 0..blocks[0] {
                let slot = x + blocks[0] * (y + blocks[1] * z);
                let block = [
                    chunk.block_origin[0] + x as i64,
                    chunk.block_origin[1] + y as i64,
                    chunk.block_origin[2] + z as i64,
                ];
                sink.field.set(block, chunk.blocks[slot]);
                sink.occupied[block[0] as usize
                    + global[0] * (block[1] as usize + global[1] * block[2] as usize)] =
                    chunk.occupied[slot];
            }
        }
    }
    Ok(())
}

struct MaskChunk {
    rows: usize,
    foreground: Vec<u64>,
    raised: Vec<u64>,
    admitted: Option<Vec<u64>>,
    blocks: Vec<(f32, f32)>,
    occupied: Vec<bool>,
    block_origin: [i64; 3],
    block_dims: [usize; 3],
}

struct MaskWorker;

impl MaskWorker {
    fn run(
        plan: &SurfaceBuilder,
        gate: &Gate<'_>,
        index: usize,
        pad: [u32; 3],
        read: &Read<'_>,
    ) -> Result<MaskChunk, String> {
        let array = plan.array_range(index, pad);
        let core = plan.core_range(index);
        let dims = dims_of(&array);
        let core_dims = dims_of(&core);
        let count = dims[0] * dims[1] * dims[2];
        let data = read(index, &array)?;
        if data.voxel_count() != count {
            return Err("the detection tile does not cover the requested region".to_string());
        }
        let mut raw = vec![0.0f32; count];
        data.write_values(&mut raw);
        smooth(&mut raw, dims, &plan.kernels);
        let start = [
            array[0].start as i64,
            array[1].start as i64,
            array[2].start as i64,
        ];
        let field = LocalField::new(
            dims,
            start,
            plan.anchor,
            plan.intensity,
            |slot| raw[slot],
            |voxel| gate.admits(voxel),
        );
        let origin = [
            (core[0].start - array[0].start) as usize,
            (core[1].start - array[1].start) as usize,
            (core[2].start - array[2].start) as usize,
        ];
        let rows = core_dims[0].div_ceil(64);
        let mut foreground = vec![0u64; rows * core_dims[1] * core_dims[2]];
        let mut raised = vec![0u64; rows * core_dims[1] * core_dims[2]];
        let mut admitted = gate
            .roi
            .is_some()
            .then(|| vec![0u64; rows * core_dims[1] * core_dims[2]]);
        let block_origin = [
            (core[0].start as i64 - plan.anchor[0]).div_euclid(BLOCK),
            (core[1].start as i64 - plan.anchor[1]).div_euclid(BLOCK),
            (core[2].start as i64 - plan.anchor[2]).div_euclid(BLOCK),
        ];
        let block_dims = [
            (core[0].end as i64 - 1 - plan.anchor[0]).div_euclid(BLOCK) - block_origin[0] + 1,
            (core[1].end as i64 - 1 - plan.anchor[1]).div_euclid(BLOCK) - block_origin[1] + 1,
            (core[2].end as i64 - 1 - plan.anchor[2]).div_euclid(BLOCK) - block_origin[2] + 1,
        ]
        .map(|value| value as usize);
        let blocks = [
            core_dims[0].div_ceil(SIDE),
            core_dims[1].div_ceil(SIDE),
            core_dims[2].div_ceil(SIDE),
        ];
        assert_eq!(block_dims, blocks, "a tile holds whole mask blocks");
        let mut occupied = vec![false; blocks[0] * blocks[1] * blocks[2]];
        let threshold = plan.threshold;
        for z in 0..core_dims[2] {
            for y in 0..core_dims[1] {
                let base = dims[0] * (origin[1] + y + dims[1] * (origin[2] + z)) + origin[0];
                let row = rows * (y + core_dims[1] * z);
                for x in 0..core_dims[0] {
                    let voxel = [
                        core[0].start as i64 + x as i64,
                        core[1].start as i64 + y as i64,
                        core[2].start as i64 + z as i64,
                    ];
                    if let Some(admitted) = admitted.as_mut() {
                        if !gate.admits(voxel) {
                            continue;
                        }
                        admitted[row + x / 64] |= 1 << (x % 64);
                    }
                    let contrast = field.contrast(voxel, raw[base + x]);
                    if contrast >= threshold {
                        foreground[row + x / 64] |= 1 << (x % 64);
                        let block = [x / SIDE, y / SIDE, z / SIDE];
                        occupied[block[0] + blocks[0] * (block[1] + blocks[1] * block[2])] = true;
                    }
                    if contrast >= -RAISED_TOLERANCE {
                        raised[row + x / 64] |= 1 << (x % 64);
                    }
                }
            }
        }
        let (field_origin, field_blocks) = field.blocks();
        let mut values = vec![(0.0f32, 0.0f32); occupied.len()];
        for z in 0..blocks[2] {
            for y in 0..blocks[1] {
                for x in 0..blocks[0] {
                    let slot = x + blocks[0] * (y + blocks[1] * z);
                    let local = [
                        (block_origin[0] + x as i64 - field_origin[0]) as usize,
                        (block_origin[1] + y as i64 - field_origin[1]) as usize,
                        (block_origin[2] + z as i64 - field_origin[2]) as usize,
                    ];
                    assert!(
                        local[0] < field_blocks[0]
                            && local[1] < field_blocks[1]
                            && local[2] < field_blocks[2],
                        "a core block lies inside the analysis window"
                    );
                    values[slot] = field.block(
                        local[0] + field_blocks[0] * (local[1] + field_blocks[1] * local[2]),
                    );
                }
            }
        }
        Ok(MaskChunk {
            rows,
            foreground,
            raised,
            admitted,
            blocks: values,
            occupied,
            block_origin,
            block_dims,
        })
    }
}

fn morph(
    plan: &SurfaceBuilder,
    workers: usize,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(usize, usize),
    foreground: &Bits,
    admitted: Option<&Bits>,
    grid: &Grid,
) -> Result<Option<Bits>, String> {
    let dims = plan.dims();
    let total = plan.tile_count();
    let step = total * 2;
    let pad = plan.pad_morph();
    let mut solid = Bits::new(dims);
    let completed = parallel_tiles(
        total,
        workers,
        cancel,
        &mut |done, _| progress(done, step),
        |index| MorphWorker::run(plan, index, pad, foreground, admitted, grid),
        |index, chunk| {
            let core = plan.core_range(index);
            let dims = dims_of(&core);
            let origin = [
                (core[0].start - plan.region[0].start) as usize,
                (core[1].start - plan.region[1].start) as usize,
                (core[2].start - plan.region[2].start) as usize,
            ];
            if chunk.foreground.len() != chunk.rows * dims[1] * dims[2] {
                return Err("a morphology tile carries the wrong number of words".to_string());
            }
            for z in 0..dims[2] {
                for y in 0..dims[1] {
                    let start = chunk.rows * (y + dims[1] * z);
                    let row = &chunk.foreground[start..start + chunk.rows];
                    solid.write_row(origin[0], origin[1] + y, origin[2] + z, row, dims[0]);
                }
            }
            Ok(())
        },
    )?;
    Ok(completed.then_some(solid))
}

struct MorphChunk {
    rows: usize,
    foreground: Vec<u64>,
}

struct MorphWorker;

impl MorphWorker {
    fn run(
        plan: &SurfaceBuilder,
        index: usize,
        pad: [u32; 3],
        foreground: &Bits,
        admitted: Option<&Bits>,
        grid: &Grid,
    ) -> Result<MorphChunk, String> {
        let array = plan.array_range(index, pad);
        let core = plan.core_range(index);
        let dims = dims_of(&array);
        let core_dims = dims_of(&core);
        let mut mask = vec![false; dims[0] * dims[1] * dims[2]];
        for z in 0..dims[2] {
            for y in 0..dims[1] {
                let base = dims[0] * (y + dims[1] * z);
                for x in 0..dims[0] {
                    let voxel = [
                        array[0].start as i64 + x as i64 - plan.anchor[0],
                        array[1].start as i64 + y as i64 - plan.anchor[1],
                        array[2].start as i64 + z as i64 - plan.anchor[2],
                    ];
                    if voxel.iter().any(|value| *value < 0) {
                        continue;
                    }
                    mask[base + x] = grid.is_solid(
                        foreground,
                        admitted,
                        voxel[0] as usize,
                        voxel[1] as usize,
                        voxel[2] as usize,
                    );
                }
            }
        }
        let mut scratch = vec![false; mask.len()];
        let mut distance = vec![0u32; mask.len()];
        if let Some(element) = &plan.opening {
            morphology::open(&mut mask, &mut scratch, &mut distance, dims, element);
        }
        if let Some(element) = &plan.closing {
            morphology::close(&mut mask, &mut scratch, &mut distance, dims, element);
        }
        let origin = [
            (core[0].start - array[0].start) as usize,
            (core[1].start - array[1].start) as usize,
            (core[2].start - array[2].start) as usize,
        ];
        let rows = core_dims[0].div_ceil(64);
        let mut foreground = vec![0u64; rows * core_dims[1] * core_dims[2]];
        for z in 0..core_dims[2] {
            for y in 0..core_dims[1] {
                let base = dims[0] * (origin[1] + y + dims[1] * (origin[2] + z)) + origin[0];
                let row = rows * (y + core_dims[1] * z);
                for x in 0..core_dims[0] {
                    if mask[base + x] {
                        foreground[row + x / 64] |= 1 << (x % 64);
                    }
                }
            }
        }
        Ok(MorphChunk { rows, foreground })
    }
}
