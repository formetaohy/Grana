use super::Gate;
use super::detection::{Detection, DetectionSource};
use crate::local::{BLOCK, BlockField, block_levels};

type Levels = Vec<Option<(f32, f32)>>;

const LEVEL_CHUNK: usize = 64;

pub(super) fn build(
    source: &DetectionSource<'_>,
    roi: Option<&grana_object::Mask>,
    progress: &mut dyn FnMut(usize, usize),
) -> Result<Option<BlockField>, String> {
    let plan = source.plan();
    let blocks = plan.blocks();
    let count = blocks[0] * blocks[1] * blocks[2];
    let mut background = vec![f32::NAN; count];
    let mut signal = vec![f32::NAN; count];
    let stride = [1, blocks[0], blocks[0] * blocks[1]];
    let gate = plan.gate(roi);
    let total = plan.dims()[2].div_ceil(BLOCK as usize);
    let mut done = 0usize;
    let chunk = (plan.tile[2] as usize).max(BLOCK as usize);
    let mut z = plan.region[2].start;
    while z < plan.region[2].end {
        if source.cancel().load(std::sync::atomic::Ordering::Relaxed) {
            return Ok(None);
        }
        let end = plan.region[2].end.min(z.saturating_add(chunk as u32));
        let Some(detection) = source.slab(z..end)? else {
            return Ok(None);
        };
        let first = ((z as i64 - plan.anchor[2]) / BLOCK) as usize;
        let last = first + (end - z).div_ceil(BLOCK as u32) as usize;
        let mut coords = Vec::with_capacity(blocks[0] * blocks[1] * (last - first));
        for bz in first..last {
            for by in 0..blocks[1] {
                for bx in 0..blocks[0] {
                    coords.push([bx, by, bz]);
                }
            }
        }
        let Some(levels) = levels(source, &gate, detection.as_ref(), &coords)? else {
            return Ok(None);
        };
        for (coord, level) in coords.iter().zip(levels) {
            let Some((background_level, signal_level)) = level else {
                continue;
            };
            let slot = stride[0] * coord[0] + stride[1] * coord[1] + stride[2] * coord[2];
            background[slot] = background_level;
            signal[slot] = signal_level;
        }
        done += last - first;
        progress(done, total);
        z = end;
    }
    Ok(Some(BlockField::from_blocks(
        plan.anchor,
        blocks,
        plan.intensity,
        background,
        signal,
        source.workers(),
    )))
}

fn levels(
    source: &DetectionSource<'_>,
    gate: &Gate<'_>,
    detection: &Detection,
    coords: &[[usize; 3]],
) -> Result<Option<Levels>, String> {
    let plan = source.plan();
    let mut output: Levels = (0..coords.len()).map(|_| None).collect();
    let chunks = coords.len().div_ceil(LEVEL_CHUNK);
    let completed = crate::schedule::ordered(
        crate::schedule::Run {
            total: chunks,
            workers: source.workers(),
            cancel: source.cancel(),
            progress: &mut |_, _| {},
        },
        || Vec::with_capacity((BLOCK * BLOCK * BLOCK) as usize),
        |bucket, chunk| {
            let first = chunk * LEVEL_CHUNK;
            let last = (first + LEVEL_CHUNK).min(coords.len());
            let mut levels = Vec::with_capacity(last - first);
            for coord in &coords[first..last] {
                levels.push(collect(plan, gate, detection, *coord, bucket));
            }
            Ok(levels)
        },
        |chunk, levels| {
            let first = chunk * LEVEL_CHUNK;
            for (offset, level) in levels.into_iter().enumerate() {
                output[first + offset] = level;
            }
            Ok(())
        },
        |_| {},
    )?;
    Ok(completed.then_some(output))
}

fn collect(
    plan: &super::SurfaceBuilder,
    gate: &Gate<'_>,
    detection: &Detection,
    block: [usize; 3],
    bucket: &mut Vec<f32>,
) -> Option<(f32, f32)> {
    bucket.clear();
    let span = [0, 1, 2].map(|axis| {
        let first = plan.anchor[axis] + (block[axis] * BLOCK as usize) as i64;
        let last = (first + BLOCK).min(i64::from(plan.region[axis].end));
        first..last
    });
    let base = detection.offset(span[0].start);
    for z in span[2].clone() {
        for y in span[1].clone() {
            let Some(line) = detection.row(y, z) else {
                continue;
            };
            for x in span[0].clone() {
                let slot = base + (x - span[0].start) as usize;
                if gate.admits([x, y, z]) {
                    bucket.push(line[slot]);
                }
            }
        }
    }
    block_levels(bucket)
}
