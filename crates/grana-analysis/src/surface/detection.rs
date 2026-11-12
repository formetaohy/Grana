use std::borrow::Cow;
use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io::{Read as _, Seek as _, SeekFrom, Write as _};
use std::mem::size_of;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, Once, mpsc};
use std::time::{Duration, SystemTime};

use super::{Gate, Read, SurfaceBuilder};
use crate::local::BlockLevels;

const BAND: u32 = 32;
const BAND_BYTES: u64 = 64 << 20;
const SCRATCH_MARGIN: u64 = 64 << 20;
const SCRATCH_STALE: Duration = Duration::from_secs(6 * 60 * 60);
const RETRY_LIMIT: u32 = 8;
const SPILL_PREFIX: &str = "grana-detection-";
const SPILL_SUFFIX: &str = ".spill";

static NEXT: AtomicU64 = AtomicU64::new(0);
static SWEEP: Once = Once::new();
static RESERVED: Mutex<u64> = Mutex::new(0);

type Band = (u32, Vec<f32>);

pub(super) struct DetectionStore {
    storage: Storage,
    start: [i64; 3],
    dims: [usize; 3],
    plane: usize,
}

pub(super) struct DetectionBuild {
    pub(super) store: DetectionStore,
    pub(super) field: crate::local::BlockField,
}

enum Storage {
    Memory(Vec<f32>),
    Spill {
        file: Mutex<File>,
        path: PathBuf,
        bytes: u64,
    },
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
        if let Storage::Spill { file, path, bytes } = storage {
            drop(file);
            let _ = std::fs::remove_file(path);
            release(bytes);
        }
    }
}

pub(super) struct DetectionInput<'a> {
    pub(super) plan: &'a SurfaceBuilder,
    pub(super) cover: &'a [Range<u32>; 3],
    pub(super) roi: Option<&'a grana_object::Mask>,
    pub(super) workers: usize,
}

struct Production<'a> {
    gate: Gate<'a>,
    levels: BlockLevels,
    kernel: &'a [f32],
    radius: usize,
    workers: usize,
}

impl DetectionStore {
    pub(super) fn build(
        input: DetectionInput<'_>,
        cancel: &AtomicBool,
        read: &Read<'_>,
        budget: u64,
        progress: &mut dyn FnMut(usize, usize),
    ) -> Result<Option<DetectionBuild>, String> {
        let DetectionInput {
            plan,
            cover,
            roi,
            workers,
        } = input;
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
            spill(bytes)?
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
        let first = cover[2].start;
        let last = cover[2].end - 1;
        let band = band_planes(plane);
        let bands = (cover[2].end - cover[2].start).div_ceil(band) as usize;
        let mut done = 0usize;
        let mut production = Production {
            gate: plan.gate(roi),
            levels: BlockLevels::new(
                plan.anchor,
                plan.region.clone(),
                cover.clone(),
                plan.intensity,
                workers.max(1),
            ),
            kernel: &plan.kernels[2],
            radius: 0,
            workers: workers.max(1),
        };
        production.radius = production.kernel.len() / 2;
        let (sender, receiver) = mpsc::sync_channel::<Result<Band, String>>(1);
        let (recycle_tx, recycle_rx) = mpsc::channel::<Vec<f32>>();
        let mut window = Window::new(plane);
        let mut available = first;
        let mut output = first;
        let completed = std::thread::scope(|scope| -> Result<bool, String> {
            {
                let read = &*read;
                let cancel = &*cancel;
                scope.spawn(move || {
                    let mut z = cover[2].start;
                    while z < cover[2].end {
                        if cancel.load(Ordering::Relaxed) {
                            return;
                        }
                        let end = cover[2].end.min(z.saturating_add(band));
                        let ranges = [cover[0].clone(), cover[1].clone(), z..end];
                        let mut raw = recycle_rx.try_recv().unwrap_or_default();
                        raw.resize(plane * (end - z) as usize, 0.0);
                        let band = match read(&ranges) {
                            Ok(data) if data.voxel_count() == raw.len() => {
                                data.write_values(&mut raw);
                                Ok((z, raw))
                            }
                            Ok(_) => Err("the detection band does not cover the requested region"
                                .to_string()),
                            Err(detail) => Err(detail),
                        };
                        let failed = band.is_err();
                        if sender.send(band).is_err() || failed {
                            return;
                        }
                        z = end;
                    }
                });
            }
            while let Ok(message) = receiver.recv() {
                let (z, mut raw) = message?;
                let planes = (raw.len() / plane) as u32;
                blur(&mut raw, plan, dims, plane, production.workers);
                window.push(z, raw);
                available = z + planes;
                let radius = production.radius;
                let ready = if available > last {
                    cover[2].end
                } else {
                    available.saturating_sub(radius as u32).max(output)
                };
                if ready > output {
                    store.write_block(&window, output, ready, &mut production)?;
                    output = ready;
                }
                for retired in window.trim(output.saturating_sub(radius as u32)) {
                    let _ = recycle_tx.send(retired);
                }
                done += 1;
                progress(done, bands);
                if cancel.load(Ordering::Relaxed) {
                    return Ok(false);
                }
            }
            if cancel.load(Ordering::Relaxed) {
                return Ok(false);
            }
            if !window.is_empty() && output < cover[2].end {
                store.write_block(&window, output, cover[2].end, &mut production)?;
            }
            Ok(true)
        })?;
        if !completed {
            return Ok(None);
        }
        Ok(Some(DetectionBuild {
            store,
            field: production.levels.finish(),
        }))
    }

    fn write_block(
        &mut self,
        window: &Window,
        start: u32,
        end: u32,
        production: &mut Production<'_>,
    ) -> Result<(), String> {
        let plane = self.plane;
        let low = self.start[2] as u32;
        let planes = (end - start) as usize;
        let radius = production.radius;
        let kernel = production.kernel;
        let workers = production.workers;
        let gate = &production.gate;
        let admits = |voxel: [i64; 3]| gate.admits(voxel);
        let levels = &mut production.levels;
        let target = |values: &mut [f32]| {
            depth(window, start, radius, kernel, workers, plane, values);
        };
        match &mut self.storage {
            Storage::Memory(store) => {
                let offset = (start - low) as usize * plane;
                let values = &mut store[offset..offset + planes * plane];
                target(values);
                levels.push(&admits, start, values, planes);
            }
            Storage::Spill { file, .. } => {
                let mut values = vec![0.0f32; planes * plane];
                target(&mut values);
                let file = file
                    .get_mut()
                    .map_err(|_| "the detection spill file is poisoned".to_string())?;
                let offset = (start - low) as u64 * plane as u64 * size_of::<f32>() as u64;
                file.seek(SeekFrom::Start(offset))
                    .map_err(|error| format!("failed to seek the detection spill: {error}"))?;
                file.write_all(bytemuck::cast_slice(&values))
                    .map_err(|error| format!("failed to write the detection spill: {error}"))?;
                levels.push(&admits, start, &values, planes);
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

fn blur(values: &mut [f32], plan: &SurfaceBuilder, dims: [usize; 3], plane: usize, workers: usize) {
    let active = plan
        .kernels
        .iter()
        .take(2)
        .enumerate()
        .filter(|(_, kernel)| kernel.len() > 1)
        .collect::<Vec<_>>();
    if active.is_empty() {
        return;
    }
    planes(values, plane, workers, |part, _| {
        for (axis, kernel) in &active {
            if *axis == 0 {
                horizontal(part, dims, part.len() / plane, kernel);
            } else {
                vertical(part, dims, part.len() / plane, kernel);
            }
        }
    });
}

fn depth(
    window: &Window,
    start: u32,
    radius: usize,
    kernel: &[f32],
    workers: usize,
    plane: usize,
    target: &mut [f32],
) {
    if target.is_empty() {
        return;
    }
    let planes = target.len() / plane;
    let workers = workers.clamp(1, planes.max(1));
    if workers == 1 {
        for (index, values) in target.chunks_mut(plane).enumerate() {
            depth_plane(window, start + index as u32, radius, kernel, values);
        }
        return;
    }
    let each = planes.div_ceil(workers);
    crate::schedule::parallel(|scope| {
        for (block, values) in target.chunks_mut(each * plane).enumerate() {
            let first = start + (block * each) as u32;
            scope.spawn(move |_| {
                for (index, plane) in values.chunks_mut(plane).enumerate() {
                    depth_plane(window, first + index as u32, radius, kernel, plane);
                }
            });
        }
    });
}

fn depth_plane(window: &Window, z: u32, radius: usize, kernel: &[f32], values: &mut [f32]) {
    values.fill(0.0);
    for (tap, weight) in kernel.iter().enumerate() {
        let source = window.plane((z as i64 + tap as i64 - radius as i64).max(0) as u32);
        for (slot, value) in values.iter_mut().zip(source) {
            *slot += weight * value;
        }
    }
}

struct Window {
    bands: VecDeque<Band>,
    plane: usize,
}

impl Window {
    fn new(plane: usize) -> Self {
        Self {
            bands: VecDeque::new(),
            plane,
        }
    }

    fn push(&mut self, z: u32, values: Vec<f32>) {
        assert!(
            !values.is_empty() && values.len().is_multiple_of(self.plane),
            "a detection band holds whole planes"
        );
        self.bands.push_back((z, values));
    }

    fn first(&self) -> u32 {
        self.bands.front().map_or(0, |(z, _)| *z)
    }

    fn is_empty(&self) -> bool {
        self.bands.is_empty()
    }

    fn last(&self) -> u32 {
        self.bands
            .back()
            .map_or(0, |(z, values)| z + (values.len() / self.plane) as u32 - 1)
    }

    fn plane(&self, z: u32) -> &[f32] {
        let z = z.clamp(self.first(), self.last());
        for (start, values) in self.bands.iter().rev() {
            let count = (values.len() / self.plane) as u32;
            if z >= *start && z < *start + count {
                let offset = (z - *start) as usize * self.plane;
                return &values[offset..offset + self.plane];
            }
        }
        unreachable!("the detection window holds every retained plane")
    }

    fn trim(&mut self, low: u32) -> Vec<Vec<f32>> {
        let mut retired = Vec::new();
        while self.bands.len() > 1 {
            let end = {
                let (start, values) = self
                    .bands
                    .front()
                    .expect("the detection window holds a band");
                start + (values.len() / self.plane) as u32
            };
            if end > low {
                break;
            }
            if let Some((_, values)) = self.bands.pop_front() {
                retired.push(values);
            }
        }
        retired
    }
}

fn spill(bytes: u64) -> Result<Storage, String> {
    let directory = std::env::temp_dir();
    SWEEP.call_once(|| sweep(&directory));
    let available = fs4::available_space(&directory).map_err(|error| {
        format!(
            "failed to measure the free space of {}: {error}",
            directory.display()
        )
    })?;
    {
        let mut reserved = RESERVED
            .lock()
            .expect("the detection spill budget is never poisoned");
        if available < *reserved + bytes + SCRATCH_MARGIN {
            return Err(format!(
                "the detection volume spills {:.2} GiB to {} and the drive holds {:.2} GiB available with {:.2} GiB already spilled",
                gib(bytes),
                directory.display(),
                gib(available),
                gib(*reserved)
            ));
        }
        *reserved += bytes;
    }
    let (path, file) = match create(&directory, bytes) {
        Ok(created) => created,
        Err(detail) => {
            release(bytes);
            return Err(detail);
        }
    };
    Ok(Storage::Spill {
        file: Mutex::new(file),
        path,
        bytes,
    })
}

fn release(bytes: u64) {
    let mut reserved = RESERVED
        .lock()
        .expect("the detection spill budget is never poisoned");
    *reserved = reserved.saturating_sub(bytes);
}

fn create(directory: &Path, bytes: u64) -> Result<(PathBuf, File), String> {
    let mut attempt = 0;
    loop {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = directory.join(format!(
            "{SPILL_PREFIX}{}-{id}{SPILL_SUFFIX}",
            std::process::id()
        ));
        match OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => match file.set_len(bytes) {
                Ok(()) => return Ok((path, file)),
                Err(error) => {
                    let _ = std::fs::remove_file(&path);
                    return Err(format!("failed to size the detection spill file: {error}"));
                }
            },
            Err(error)
                if error.kind() == std::io::ErrorKind::AlreadyExists && attempt < RETRY_LIMIT =>
            {
                attempt += 1;
            }
            Err(error) => {
                return Err(format!(
                    "failed to open the detection spill file {}: {error}",
                    path.display()
                ));
            }
        }
    }
}

fn sweep(directory: &Path) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    let now = SystemTime::now();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with(SPILL_PREFIX) || !name.ends_with(SPILL_SUFFIX) {
            continue;
        }
        let stale = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age > SCRATCH_STALE);
        if stale {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

fn gib(bytes: u64) -> f64 {
    bytes as f64 / (1u64 << 30) as f64
}

fn band_planes(plane: usize) -> u32 {
    let bytes = (plane as u64 * size_of::<f32>() as u64).max(1);
    (BAND_BYTES / bytes).clamp(1, u64::from(BAND)) as u32
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
    crate::schedule::parallel(|scope| {
        for (index, chunk) in values.chunks_mut(each * plane).enumerate() {
            let first = index * each;
            scope.spawn(move |_| body(chunk, first));
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
            blur_line(&padded, &mut blurred, kernel, radius);
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

fn blur_line(padded: &[f32], blurred: &mut [f32], kernel: &[f32], radius: usize) {
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
