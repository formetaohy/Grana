use std::borrow::Cow;
use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io::{Read as _, Seek as _, SeekFrom, Write as _};
use std::mem::size_of;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use super::{Read, SurfaceBuilder};

const BAND: u32 = 32;

static NEXT: AtomicU64 = AtomicU64::new(0);

pub(super) struct DetectionStore {
    storage: Storage,
    start: [i64; 3],
    dims: [usize; 3],
    plane: usize,
}

enum Storage {
    Memory(Vec<f32>),
    Spill { file: Mutex<File>, path: PathBuf },
}

pub(super) struct DetectionSource<'a> {
    plan: &'a SurfaceBuilder,
    cover: &'a [Range<u32>; 3],
    workers: usize,
    cancel: &'a AtomicBool,
    store: &'a DetectionStore,
}

pub(super) struct Detection<'a> {
    values: Cow<'a, [f32]>,
    start: [i64; 3],
    dims: [usize; 3],
    strides: [usize; 3],
}

impl Drop for DetectionStore {
    fn drop(&mut self) {
        let storage = std::mem::replace(&mut self.storage, Storage::Memory(Vec::new()));
        if let Storage::Spill { file, path } = storage {
            drop(file);
            let _ = std::fs::remove_file(path);
        }
    }
}

impl DetectionStore {
    pub(super) fn build(
        plan: &SurfaceBuilder,
        cover: &[Range<u32>; 3],
        workers: usize,
        cancel: &AtomicBool,
        read: &Read<'_>,
        budget: u64,
        progress: &mut dyn FnMut(usize, usize),
    ) -> Result<Option<Self>, String> {
        let dims = super::dims_of(cover);
        if dims.contains(&0) || cover[2].is_empty() {
            return Ok(None);
        }
        let plane = dims[0] * dims[1];
        let total = plane * dims[2];
        let bytes = total as u64 * size_of::<f32>() as u64;
        let storage = if bytes <= budget {
            Storage::Memory(vec![0.0; total])
        } else {
            let path = spill_path();
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .open(&path)
                .map_err(|error| format!("failed to open the detection spill file: {error}"))?;
            file.set_len(bytes).map_err(|error| {
                let _ = std::fs::remove_file(&path);
                format!("failed to size the detection spill file: {error}")
            })?;
            Storage::Spill {
                file: Mutex::new(file),
                path,
            }
        };
        let mut store = Self {
            storage,
            start: [
                i64::from(cover[0].start),
                i64::from(cover[1].start),
                i64::from(cover[2].start),
            ],
            dims,
            plane,
        };
        let radius = plan.kernels[2].len() / 2;
        let mut ring: VecDeque<Vec<f32>> = VecDeque::with_capacity(2 * radius + 1);
        let mut smoothed = vec![0.0f32; plane];
        let mut output = cover[2].start;
        let mut z = cover[2].start;
        let bands = (cover[2].end - cover[2].start).div_ceil(BAND) as usize;
        let mut done = 0usize;
        let mut first = true;
        let mut last = Vec::new();
        while z < cover[2].end {
            if cancel.load(Ordering::Relaxed) {
                return Ok(None);
            }
            let end = cover[2].end.min(z.saturating_add(BAND));
            let mut raw = vec![0.0f32; plane * (end - z) as usize];
            let ranges = [cover[0].clone(), cover[1].clone(), z..end];
            let data = read(&ranges)?;
            if data.voxel_count() != raw.len() {
                return Err("the detection band does not cover the requested region".to_string());
            }
            data.write_values(&mut raw);
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
            }
            for local in 0..(end - z) as usize {
                let start = local * plane;
                let source = &raw[start..start + plane];
                if first {
                    first = false;
                    for _ in 0..radius {
                        ring.push_back(source.to_vec());
                    }
                }
                ring.push_back(source.to_vec());
                last.clear();
                last.extend_from_slice(source);
                while ring.len() > 2 * radius {
                    depth(&ring, &plan.kernels[2], radius, &mut smoothed);
                    store.write_plane(output, &smoothed)?;
                    output += 1;
                    ring.pop_front();
                }
            }
            done += 1;
            progress(done, bands);
            z = end;
        }
        let full = 2 * radius + 1;
        while output < cover[2].end {
            if ring.len() < full {
                ring.push_back(last.clone());
                continue;
            }
            if output > cover[2].start {
                ring.pop_front();
                ring.push_back(last.clone());
            }
            depth(&ring, &plan.kernels[2], radius, &mut smoothed);
            store.write_plane(output, &smoothed)?;
            output += 1;
        }
        assert_eq!(
            output, cover[2].end,
            "the detection store covers every plane"
        );
        Ok(Some(store))
    }

    fn write_plane(&mut self, z: u32, values: &[f32]) -> Result<(), String> {
        let offset = (z - self.start[2] as u32) as usize * self.plane;
        match &mut self.storage {
            Storage::Memory(store) => {
                store[offset..offset + values.len()].copy_from_slice(values);
            }
            Storage::Spill { file, .. } => {
                let file = file
                    .get_mut()
                    .map_err(|_| "the detection spill file is poisoned".to_string())?;
                file.seek(SeekFrom::Start(offset as u64 * 4))
                    .map_err(|error| format!("failed to seek the detection spill: {error}"))?;
                file.write_all(bytemuck::cast_slice(values))
                    .map_err(|error| format!("failed to write the detection spill: {error}"))?;
            }
        }
        Ok(())
    }

    pub(super) fn whole(&self) -> Option<Detection<'_>> {
        let Storage::Memory(values) = &self.storage else {
            return None;
        };
        Some(Detection {
            values: Cow::Borrowed(&values[..]),
            start: self.start,
            dims: self.dims,
            strides: super::strides_of(self.dims),
        })
    }

    pub(super) fn slab(&self, z: Range<u32>) -> Result<Option<Detection<'_>>, String> {
        let low = self.start[2] as u32;
        let high = low + self.dims[2] as u32;
        if z.is_empty() || z.start < low || z.end > high {
            return Ok(None);
        }
        let offset = (z.start - low) as usize * self.plane;
        let length = (z.end - z.start) as usize * self.plane;
        let dims = [self.dims[0], self.dims[1], (z.end - z.start) as usize];
        let values = match &self.storage {
            Storage::Memory(store) => Cow::Borrowed(&store[offset..offset + length]),
            Storage::Spill { file, .. } => {
                let mut values = vec![0.0f32; length];
                let bytes = bytemuck::cast_slice_mut(&mut values);
                let mut file = file
                    .lock()
                    .map_err(|_| "the detection spill file is poisoned".to_string())?;
                file.seek(SeekFrom::Start(offset as u64 * 4))
                    .map_err(|error| format!("failed to seek the detection spill: {error}"))?;
                file.read_exact(bytes)
                    .map_err(|error| format!("failed to read the detection spill: {error}"))?;
                drop(file);
                Cow::Owned(values)
            }
        };
        Ok(Some(Detection {
            values,
            start: [self.start[0], self.start[1], i64::from(z.start)],
            dims,
            strides: super::strides_of(dims),
        }))
    }
}

impl<'a> DetectionSource<'a> {
    pub(super) fn new(
        plan: &'a SurfaceBuilder,
        cover: &'a [Range<u32>; 3],
        workers: usize,
        cancel: &'a AtomicBool,
        store: &'a DetectionStore,
    ) -> Self {
        Self {
            plan,
            cover,
            workers,
            cancel,
            store,
        }
    }

    pub(super) fn plan(&self) -> &'a SurfaceBuilder {
        self.plan
    }

    pub(super) fn cover(&self) -> &'a [Range<u32>; 3] {
        self.cover
    }

    pub(super) fn whole(&self) -> Option<Detection<'a>> {
        self.store.whole()
    }

    pub(super) fn workers(&self) -> usize {
        self.workers
    }

    pub(super) fn cancel(&self) -> &'a AtomicBool {
        self.cancel
    }

    pub(super) fn slab(&self, z: Range<u32>) -> Result<Option<Detection<'_>>, String> {
        self.store.slab(z)
    }
}

impl Detection<'_> {
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

fn depth(ring: &VecDeque<Vec<f32>>, kernel: &[f32], radius: usize, values: &mut [f32]) {
    if radius == 0 {
        values.copy_from_slice(&ring[0]);
        return;
    }
    values.fill(0.0);
    for (tap, weight) in kernel.iter().enumerate() {
        let source = &ring[tap];
        for (slot, value) in values.iter_mut().zip(source) {
            *slot += weight * value;
        }
    }
}

fn spill_path() -> PathBuf {
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("grana-detection-{}-{id}.spill", std::process::id()))
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

#[cfg(test)]
mod tests {
    use super::*;
    use grana_object::{SurfaceFilter, SurfaceParams};
    use grana_volume::{Extent, Intensity, Level, Region, Spacing, VoxelData};

    #[test]
    fn reads_and_smooths_a_spilled_detection_once() {
        let level = Level::new(Extent::new(32, 24, 6), Spacing::new(1.0, 1.0, 1.0));
        let params = SurfaceParams::new(0.5, 1.0, 0.0, 0.0, SurfaceFilter::new(0.0, 0.0, 0.0, 0.0));
        let plan = SurfaceBuilder::new(
            0,
            level,
            params,
            1,
            0,
            Region::none(),
            Intensity::new(0.0, 1.0),
        );
        let cover = plan.coverage(None);
        let volume = (cover[0].end - cover[0].start) as u64
            * (cover[1].end - cover[1].start) as u64
            * (cover[2].end - cover[2].start) as u64;
        let reads = std::sync::atomic::AtomicU64::new(0);
        let read = |ranges: &[Range<u32>; 3]| {
            let count = (ranges[0].end - ranges[0].start) as usize
                * (ranges[1].end - ranges[1].start) as usize
                * (ranges[2].end - ranges[2].start) as usize;
            reads.fetch_add(count as u64, Ordering::Relaxed);
            Ok(VoxelData::F32(vec![0.25; count]))
        };
        let cancel = AtomicBool::new(false);
        let mut progress = |_, _| {};
        let memory =
            DetectionStore::build(&plan, &cover, 2, &cancel, &read, u64::MAX, &mut progress)
                .expect("the memory detection builds")
                .expect("the memory detection covers voxels");
        assert_eq!(reads.load(Ordering::Relaxed), volume);
        reads.store(0, Ordering::Relaxed);
        let spill = DetectionStore::build(&plan, &cover, 2, &cancel, &read, 0, &mut progress)
            .expect("the spilled detection builds")
            .expect("the spilled detection covers voxels");
        assert_eq!(
            reads.load(Ordering::Relaxed),
            volume,
            "a spilled detection reads its channel once"
        );
        assert!(matches!(spill.storage, Storage::Spill { .. }));
        let slab = cover[2].clone();
        let memory = memory.slab(slab.clone()).expect("the memory slab").unwrap();
        let spilled = spill.slab(slab).expect("the spilled slab").unwrap();
        assert_eq!(
            memory.values, spilled.values,
            "the spilled detection smooths like the resident one"
        );
    }
}
