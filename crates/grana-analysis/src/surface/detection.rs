use std::ops::Range;
use std::sync::atomic::{AtomicBool, Ordering};

use super::{Read, SurfaceBuilder};

pub(super) struct Detection {
    values: Vec<f32>,
    start: [i64; 3],
    dims: [usize; 3],
    strides: [usize; 3],
}

pub(super) struct DetectionSource<'a> {
    plan: &'a SurfaceBuilder,
    cover: &'a [Range<u32>; 3],
    workers: usize,
    cancel: &'a AtomicBool,
    read: &'a Read<'a>,
    resident: Option<&'a Detection>,
}

pub(super) enum DetectionSlab<'a> {
    Resident(&'a Detection),
    Owned(Detection),
}

impl DetectionSlab<'_> {
    pub(super) fn as_ref(&self) -> &Detection {
        match self {
            Self::Resident(detection) => detection,
            Self::Owned(detection) => detection,
        }
    }
}

impl<'a> DetectionSource<'a> {
    pub(super) fn new(
        plan: &'a SurfaceBuilder,
        cover: &'a [Range<u32>; 3],
        workers: usize,
        cancel: &'a AtomicBool,
        read: &'a Read<'a>,
        resident: Option<&'a Detection>,
    ) -> Self {
        Self {
            plan,
            cover,
            workers,
            cancel,
            read,
            resident,
        }
    }

    pub(super) fn plan(&self) -> &'a SurfaceBuilder {
        self.plan
    }

    pub(super) fn cover(&self) -> &'a [Range<u32>; 3] {
        self.cover
    }

    pub(super) fn resident(&self) -> Option<&'a Detection> {
        self.resident
    }

    pub(super) fn workers(&self) -> usize {
        self.workers
    }

    pub(super) fn cancel(&self) -> &'a AtomicBool {
        self.cancel
    }

    pub(super) fn slab(&self, z: Range<u32>) -> Result<Option<DetectionSlab<'a>>, String> {
        if let Some(detection) = self.resident {
            return Ok(Some(DetectionSlab::Resident(detection)));
        }
        Ok(Detection::build(
            self.plan,
            self.cover,
            z,
            self.workers,
            self.cancel,
            self.read,
        )?
        .map(DetectionSlab::Owned))
    }
}

impl Detection {
    pub(super) fn bytes(cover: &[Range<u32>; 3]) -> u64 {
        let dims = super::dims_of(cover);
        dims[0] as u64 * dims[1] as u64 * dims[2] as u64 * 4
    }

    pub(super) fn build(
        plan: &SurfaceBuilder,
        cover: &[Range<u32>; 3],
        z: Range<u32>,
        workers: usize,
        cancel: &AtomicBool,
        read: &Read<'_>,
    ) -> Result<Option<Self>, String> {
        let dims = super::dims_of(cover);
        let plane = dims[0] * dims[1];
        if plane == 0 || z.is_empty() {
            return Ok(None);
        }
        let radius = plan.kernels[2].len() / 2;
        let raw_start = z.start.saturating_sub(radius as u32).max(cover[2].start);
        let raw_end = z.end.saturating_add(radius as u32).min(cover[2].end);
        assert!(
            raw_start <= z.start && z.end <= raw_end,
            "a detection slab holds the smoothing support of its output"
        );
        let raw_dims = [dims[0], dims[1], (raw_end - raw_start) as usize];
        let mut raw = vec![0.0f32; raw_dims[0] * raw_dims[1] * raw_dims[2]];
        let ranges = [cover[0].clone(), cover[1].clone(), raw_start..raw_end];
        let data = read(&ranges)?;
        if data.voxel_count() != raw.len() {
            return Err("the detection slab does not cover the requested region".to_string());
        }
        data.write_values(&mut raw);
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        for (axis, kernel) in plan.kernels.iter().enumerate().take(2) {
            if kernel.len() <= 1 {
                continue;
            }
            planes(&mut raw, plane, workers.max(1), |part, _| {
                if axis == 0 {
                    horizontal(part, dims, part.len() / plane, kernel);
                } else {
                    vertical(part, dims, part.len() / plane, kernel);
                }
            });
            if cancel.load(Ordering::Relaxed) {
                return Ok(None);
            }
        }
        let output = z.end - z.start;
        let mut values = vec![0.0f32; plane * output as usize];
        let kernel = &plan.kernels[2];
        if kernel.len() <= 1 {
            let offset = (z.start - raw_start) as usize * plane;
            let length = values.len();
            values.copy_from_slice(&raw[offset..offset + length]);
            return Ok(Some(Self {
                values,
                start: [
                    i64::from(cover[0].start),
                    i64::from(cover[1].start),
                    i64::from(z.start),
                ],
                dims: [dims[0], dims[1], output as usize],
                strides: super::strides_of([dims[0], dims[1], output as usize]),
            }));
        }
        let radius = kernel.len() / 2;
        planes(&mut values, plane, workers.max(1), |part, first| {
            depth(
                part,
                &raw,
                plane,
                first + z.start as usize - raw_start as usize,
                raw_dims[2],
                radius,
                kernel,
            );
        });
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        Ok(Some(Self {
            values,
            start: [
                i64::from(cover[0].start),
                i64::from(cover[1].start),
                i64::from(z.start),
            ],
            dims: [dims[0], dims[1], output as usize],
            strides: super::strides_of([dims[0], dims[1], output as usize]),
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

fn planes(
    values: &mut [f32],
    plane: usize,
    workers: usize,
    body: impl Fn(&mut [f32], usize) + Sync,
) {
    if plane == 0 || values.is_empty() {
        return;
    }
    let available = values.len().div_ceil(plane);
    let workers = workers.clamp(1, available.max(1));
    let each = available.div_ceil(workers).max(1);
    if workers == 1 {
        body(values, 0);
        return;
    }
    let body = &body;
    std::thread::scope(|scope| {
        for (index, chunk) in values.chunks_mut(each * plane).enumerate() {
            let first = index * each;
            scope.spawn(move || body(chunk, first));
        }
    });
}

fn horizontal(part: &mut [f32], dims: [usize; 3], depth: usize, kernel: &[f32]) {
    let plane = dims[0] * dims[1];
    let radius = kernel.len() / 2;
    let last = dims[0] - 1;
    let mut line = vec![0.0f32; dims[0]];
    let mut blurred = vec![0.0f32; dims[0]];
    let mut padded = vec![0.0f32; dims[0] + 2 * radius];
    for z in 0..depth {
        let base = plane * z;
        for y in 0..dims[1] {
            let start = base + dims[0] * y;
            line.copy_from_slice(&part[start..start + dims[0]]);
            padded[..radius].fill(line[0]);
            padded[radius..radius + dims[0]].copy_from_slice(&line);
            padded[radius + dims[0]..].fill(line[last]);
            blur(&padded, &mut blurred, kernel, radius);
            part[start..start + dims[0]].copy_from_slice(&blurred);
        }
    }
}

const BLOCK: usize = 32;

fn vertical(part: &mut [f32], dims: [usize; 3], depth: usize, kernel: &[f32]) {
    let plane = dims[0] * dims[1];
    let radius = (kernel.len() / 2) as i64;
    let last = dims[1] as i64 - 1;
    let mut block = vec![0.0f32; dims[1] * BLOCK];
    let mut blurred = vec![0.0f32; dims[1] * BLOCK];
    for z in 0..depth {
        let base = plane * z;
        let mut x0 = 0;
        while x0 < dims[0] {
            let width = (dims[0] - x0).min(BLOCK);
            for y in 0..dims[1] {
                let start = base + y * dims[0] + x0;
                block[y * width..(y + 1) * width].copy_from_slice(&part[start..start + width]);
            }
            for y in 0..dims[1] {
                let target = &mut blurred[y * width..(y + 1) * width];
                target.fill(0.0);
                for (tap, weight) in kernel.iter().enumerate() {
                    let sample = (y as i64 + tap as i64 - radius).clamp(0, last) as usize;
                    let source = &block[sample * width..(sample + 1) * width];
                    for (slot, value) in target.iter_mut().zip(source) {
                        *slot += weight * value;
                    }
                }
            }
            for y in 0..dims[1] {
                let start = base + y * dims[0] + x0;
                part[start..start + width].copy_from_slice(&blurred[y * width..(y + 1) * width]);
            }
            x0 += width;
        }
    }
}

fn depth(
    part: &mut [f32],
    raw: &[f32],
    plane: usize,
    start: usize,
    length: usize,
    radius: usize,
    kernel: &[f32],
) {
    for z in 0..part.len() / plane {
        let output = &mut part[z * plane..(z + 1) * plane];
        output.fill(0.0);
        let centre = start + z;
        for (tap, weight) in kernel.iter().enumerate() {
            let reached =
                (centre as i64 + tap as i64 - radius as i64).clamp(0, length as i64 - 1) as usize;
            let source = &raw[reached * plane..(reached + 1) * plane];
            for (slot, value) in output.iter_mut().zip(source) {
                *slot += weight * value;
            }
        }
    }
}

fn blur(padded: &[f32], blurred: &mut [f32], kernel: &[f32], radius: usize) {
    let length = blurred.len();
    for (x, slot) in blurred.iter_mut().enumerate() {
        let source = &padded[x..x + 2 * radius + 1];
        let mut sum = 0.0f32;
        for (value, weight) in source.iter().zip(kernel) {
            sum += value * weight;
        }
        *slot = sum;
    }
    assert!(
        padded.len() >= length + 2 * radius,
        "a padded line holds its halo"
    );
}
