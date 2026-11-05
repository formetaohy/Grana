use std::ops::Range;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;

use grana_object::Mask;

use super::{Read, SurfaceBuilder, band, dims_of, strides_of};

const READ_VOXELS: u64 = 8 << 20;
const SCRATCH_BUDGET: u64 = 256 << 20;
const SAMPLE_BYTES: u64 = 4;

pub(super) struct Detection {
    values: Vec<f32>,
    start: [i64; 3],
    dims: [usize; 3],
    strides: [usize; 3],
}

impl Detection {
    pub(super) fn build(
        plan: &SurfaceBuilder,
        workers: usize,
        cancel: &AtomicBool,
        roi: Option<&Mask>,
        read: &Read<'_>,
        progress: &mut dyn FnMut(usize, usize),
        step: usize,
    ) -> Result<Option<Self>, String> {
        let cover = plan.coverage(roi);
        let dims = dims_of(&cover);
        let strides = strides_of(dims);
        let plane = dims[0] * dims[1];
        let mut values = vec![0.0f32; plane * dims[2]];
        let minimum = ((plane * dims[2]) as u64).div_ceil(READ_VOXELS);
        let slabs = bands(dims[2], workers, minimum as usize);
        run(
            &mut values,
            plane,
            &slabs,
            &mut band(progress, step, 0, 20),
            |_, part, slab| {
                if cancel.load(Ordering::Relaxed) {
                    return Ok(());
                }
                let ranges = [0, 1, 2].map(|axis| {
                    let (first, last) = if axis == 2 {
                        (slab.start, slab.end)
                    } else {
                        (0, dims[axis])
                    };
                    (cover[axis].start + first as u32)..(cover[axis].start + last as u32)
                });
                let data = read(&ranges)?;
                if data.voxel_count() != part.len() {
                    return Err(
                        "the detection slab does not cover the requested region".to_string()
                    );
                }
                data.write_values(part);
                Ok(())
            },
        )?;
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        for (axis, from) in [(0, 20), (1, 25)] {
            let kernel = &plan.kernels[axis];
            if kernel.len() <= 1 {
                continue;
            }
            let slabs = bands(dims[2], workers, 1);
            run(
                &mut values,
                plane,
                &slabs,
                &mut band(progress, step, from, from + 5),
                |_, part, slab| {
                    if cancel.load(Ordering::Relaxed) {
                        return Ok(());
                    }
                    horizontal(part, dims, slab.end - slab.start, kernel, axis);
                    Ok(())
                },
            )?;
            if cancel.load(Ordering::Relaxed) {
                return Ok(None);
            }
        }
        let kernel = &plan.kernels[2];
        if kernel.len() > 1 {
            let radius = kernel.len() / 2;
            let workers = depth_workers(plane, radius, workers);
            let slabs = bands(dims[2], workers, 1);
            let halos = halos(&values, plane, dims[2], &slabs, radius);
            run(
                &mut values,
                plane,
                &slabs,
                &mut band(progress, step, 30, 45),
                |index, part, slab| {
                    if cancel.load(Ordering::Relaxed) {
                        return Ok(());
                    }
                    depth(
                        part,
                        &halos[index],
                        plane,
                        slab.start..slab.end,
                        dims[2],
                        radius,
                        kernel,
                    );
                    Ok(())
                },
            )?;
            if cancel.load(Ordering::Relaxed) {
                return Ok(None);
            }
        }
        Ok(Some(Self {
            values,
            start: [
                i64::from(cover[0].start),
                i64::from(cover[1].start),
                i64::from(cover[2].start),
            ],
            dims,
            strides,
        }))
    }

    pub(super) fn offset(&self, x: i64) -> usize {
        (x - self.start[0]) as usize
    }

    pub(super) fn value(&self, voxel: [i64; 3]) -> Option<f32> {
        let mut local = [0usize; 3];
        for axis in 0..3 {
            let value = voxel[axis] - self.start[axis];
            if value < 0 || value >= self.dims[axis] as i64 {
                return None;
            }
            local[axis] = value as usize;
        }
        let slot =
            self.strides[0] * local[0] + self.strides[1] * local[1] + self.strides[2] * local[2];
        Some(self.values[slot])
    }

    pub(super) fn row(&self, y: i64, z: i64) -> Option<&[f32]> {
        let y = y - self.start[1];
        let z = z - self.start[2];
        if y < 0 || z < 0 || y >= self.dims[1] as i64 || z >= self.dims[2] as i64 {
            return None;
        }
        let base = self.strides[1] * y as usize + self.strides[2] * z as usize;
        Some(&self.values[base..base + self.dims[0]])
    }
}

fn bands(length: usize, workers: usize, minimum: usize) -> Vec<Range<usize>> {
    let count = length
        .clamp(1, workers.max(1))
        .max(minimum.clamp(1, length.max(1)));
    (0..count)
        .map(|index| (index * length / count)..((index + 1) * length / count))
        .filter(|range| range.end > range.start)
        .collect()
}

fn depth_workers(plane: usize, radius: usize, workers: usize) -> usize {
    let per_worker = SAMPLE_BYTES * (radius.max(1) + 1) as u64 * plane.max(1) as u64;
    ((SCRATCH_BUDGET / per_worker.max(1)) as usize).clamp(1, workers.max(1))
}

fn run(
    values: &mut [f32],
    plane: usize,
    slabs: &[Range<usize>],
    progress: &mut dyn FnMut(usize, usize),
    work: impl Fn(usize, &mut [f32], &Range<usize>) -> Result<(), String> + Sync,
) -> Result<(), String> {
    let total = slabs.len();
    let mut parts = Vec::with_capacity(total);
    let mut rest = values;
    for slab in slabs {
        let length = (slab.end - slab.start) * plane;
        let (part, tail) = rest.split_at_mut(length);
        parts.push(part);
        rest = tail;
    }
    let (sender, receiver) = mpsc::sync_channel::<Option<String>>(total.max(1));
    let failure = std::thread::scope(|scope| {
        let work = &work;
        for (index, (part, slab)) in parts.into_iter().zip(slabs).enumerate() {
            let sender = sender.clone();
            scope.spawn(move || {
                let _ = sender.send(work(index, part, slab).err());
            });
        }
        drop(sender);
        let mut failure: Option<String> = None;
        let mut done = 0usize;
        while let Ok(detail) = receiver.recv() {
            done += 1;
            progress(done, total);
            if let Some(detail) = detail {
                failure.get_or_insert(detail);
            }
        }
        failure
    });
    match failure {
        Some(detail) => Err(detail),
        None => Ok(()),
    }
}

fn horizontal(part: &mut [f32], dims: [usize; 3], depth: usize, kernel: &[f32], axis: usize) {
    let plane = dims[0] * dims[1];
    let radius = (kernel.len() / 2) as i64;
    let last = dims[axis] as i64 - 1;
    let mut line = vec![0.0f32; dims[axis]];
    let mut blurred = vec![0.0f32; dims[axis]];
    for z in 0..depth {
        let base = plane * z;
        if axis == 0 {
            for y in 0..dims[1] {
                let start = base + dims[0] * y;
                line.copy_from_slice(&part[start..start + dims[0]]);
                blur(&line, &mut blurred, kernel, radius, last);
                part[start..start + dims[0]].copy_from_slice(&blurred);
            }
        } else {
            for x in 0..dims[0] {
                let start = base + x;
                for (step, slot) in line.iter_mut().enumerate() {
                    *slot = part[start + dims[0] * step];
                }
                blur(&line, &mut blurred, kernel, radius, last);
                for (step, value) in blurred.iter().enumerate() {
                    part[start + dims[0] * step] = *value;
                }
            }
        }
    }
}

fn blur(line: &[f32], blurred: &mut [f32], kernel: &[f32], radius: i64, last: i64) {
    for (step, slot) in blurred.iter_mut().enumerate() {
        let mut sum = 0.0f32;
        for (tap, weight) in kernel.iter().enumerate() {
            let sample = (step as i64 + tap as i64 - radius).clamp(0, last) as usize;
            sum += line[sample] * weight;
        }
        *slot = sum;
    }
}

fn halos(
    values: &[f32],
    plane: usize,
    length: usize,
    slabs: &[Range<usize>],
    radius: usize,
) -> Vec<Vec<f32>> {
    let mut buffers: Vec<Vec<f32>> = slabs
        .iter()
        .map(|slab| vec![0.0f32; halo_planes(slab, length, radius) * plane])
        .collect();
    std::thread::scope(|scope| {
        for (slab, buffer) in slabs.iter().zip(buffers.iter_mut()) {
            let buffer = &mut buffer[..];
            scope.spawn(move || {
                let low = radius.min(slab.start);
                let high = radius.min(length - slab.end);
                let lower = (slab.start - low) * plane..slab.start * plane;
                let upper = slab.end * plane..(slab.end + high) * plane;
                let split = lower.len();
                buffer[..split].copy_from_slice(&values[lower]);
                buffer[split..].copy_from_slice(&values[upper]);
            });
        }
    });
    buffers
}

fn halo_planes(slab: &Range<usize>, length: usize, radius: usize) -> usize {
    radius.min(slab.start) + radius.min(length - slab.end)
}

fn depth(
    part: &mut [f32],
    halo: &[f32],
    plane: usize,
    range: Range<usize>,
    length: usize,
    radius: usize,
    kernel: &[f32],
) {
    let start = range.start;
    let depth = range.end - range.start;
    let low = radius.min(start);
    let stride = radius.max(1);
    let mut window = vec![0.0f32; stride * plane];
    let mut out = vec![0.0f32; plane];
    for z in 0..depth {
        out.fill(0.0);
        for (tap, weight) in kernel.iter().enumerate() {
            let reached = (start as i64 + z as i64 + tap as i64 - radius as i64)
                .clamp(0, length as i64 - 1) as usize;
            let source = if reached < start {
                let slot = reached - (start - low);
                &halo[slot * plane..(slot + 1) * plane]
            } else if reached >= start + depth {
                let slot = low + (reached - (start + depth));
                &halo[slot * plane..(slot + 1) * plane]
            } else if reached < start + z {
                let slot = (reached - start) % stride;
                &window[slot * plane..(slot + 1) * plane]
            } else {
                let slot = reached - start;
                &part[slot * plane..(slot + 1) * plane]
            };
            for (slot, value) in out.iter_mut().zip(source) {
                *slot += weight * value;
            }
        }
        let slot = z % stride;
        window[slot * plane..(slot + 1) * plane].copy_from_slice(&part[z * plane..(z + 1) * plane]);
        part[z * plane..(z + 1) * plane].copy_from_slice(&out);
    }
}
