use std::sync::Arc;

use grana_volume::{Extent, VoxelData, VoxelFormat};

const SOURCE_BUDGET: u64 = 512 << 20;
const TOTAL_BUDGET: u64 = 1536 << 20;
const MINIMUM_BUDGET: u64 = 48 << 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Key {
    pub(crate) level: u32,
    pub(crate) time: u32,
    pub(crate) z: u32,
}

pub(crate) struct Plane {
    channels: Vec<VoxelData>,
}

impl Plane {
    pub(crate) fn new(channels: Vec<VoxelData>) -> Self {
        assert!(!channels.is_empty(), "a plane carries at least one channel");
        Self { channels }
    }

    pub(crate) fn channel(&self, index: usize) -> &VoxelData {
        assert!(index < self.channels.len(), "a plane holds every channel");
        &self.channels[index]
    }

    fn bytes(&self) -> u64 {
        self.channels.iter().map(VoxelData::bytes).sum()
    }
}

pub(crate) struct Caches {
    levels: Vec<std::sync::Mutex<Inner>>,
    budgets: Vec<u64>,
}

struct Inner {
    entries: std::collections::HashMap<u32, Arc<Plane>>,
    order: std::collections::VecDeque<u32>,
    bytes: u64,
}

impl Caches {
    pub(crate) fn of(pyramid: &grana_volume::Pyramid) -> Self {
        let stored = pyramid.stored();
        let channel_bytes = pyramid
            .channels()
            .iter()
            .map(|channel| channel.format().bytes() as u64)
            .sum::<u64>();
        let mut remaining = TOTAL_BUDGET;
        let budgets = pyramid
            .levels()
            .iter()
            .enumerate()
            .map(|(level, info)| {
                let extent = info.extent();
                let wanted = if level < stored {
                    SOURCE_BUDGET
                } else {
                    let plane = u64::from(extent.x()) * u64::from(extent.y()) * channel_bytes;
                    plane * u64::from(extent.z())
                };
                let budget = wanted.min(remaining).max(MINIMUM_BUDGET);
                remaining = remaining.saturating_sub(budget);
                budget
            })
            .collect();
        Self {
            levels: pyramid
                .levels()
                .iter()
                .map(|_| {
                    std::sync::Mutex::new(Inner {
                        entries: std::collections::HashMap::new(),
                        order: std::collections::VecDeque::new(),
                        bytes: 0,
                    })
                })
                .collect(),
            budgets,
        }
    }

    pub(crate) fn fetch(&self, key: Key) -> Option<Arc<Plane>> {
        let mut inner = self.lock(key.level as usize);
        let plane = inner.entries.get(&key.z).cloned()?;
        inner.order.retain(|held| *held != key.z);
        inner.order.push_back(key.z);
        Some(plane)
    }

    pub(crate) fn insert(&self, key: Key, plane: Arc<Plane>) {
        let budget = self.budgets[key.level as usize];
        let mut inner = self.lock(key.level as usize);
        if inner.entries.insert(key.z, Arc::clone(&plane)).is_none() {
            inner.bytes += plane.bytes();
        }
        inner.order.push_back(key.z);
        while inner.bytes > budget {
            let Some(victim) = inner.order.pop_front() else {
                break;
            };
            if let Some(held) = inner.entries.remove(&victim) {
                inner.bytes = inner.bytes.saturating_sub(held.bytes());
            }
        }
    }

    fn lock(&self, level: usize) -> std::sync::MutexGuard<'_, Inner> {
        self.levels[level]
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }
}

pub(crate) fn blank_sums(channels: usize, voxels: usize) -> Vec<Vec<f64>> {
    (0..channels).map(|_| vec![0.0; voxels]).collect()
}

pub(crate) fn blank_counts(voxels: usize) -> Vec<u32> {
    vec![0; voxels]
}

pub(crate) fn add(data: &VoxelData, extent: Extent, ratio: u32, width: usize, sums: &mut [f64]) {
    macro_rules! arm {
        ($values:expr, $cast:expr) => {
            blocks($values, extent, ratio, width, $cast, |index, total| {
                sums[index] += total
            })
        };
    }
    match data {
        VoxelData::U8(values) => arm!(values, |value: u8| f64::from(value)),
        VoxelData::U16(values) => arm!(values, |value: u16| f64::from(value)),
        VoxelData::U32(values) => arm!(values, |value: u32| f64::from(value)),
        VoxelData::U64(values) => arm!(values, |value: u64| value as f64),
        VoxelData::I8(values) => arm!(values, |value: i8| f64::from(value)),
        VoxelData::I16(values) => arm!(values, |value: i16| f64::from(value)),
        VoxelData::I32(values) => arm!(values, |value: i32| f64::from(value)),
        VoxelData::I64(values) => arm!(values, |value: i64| value as f64),
        VoxelData::F32(values) => arm!(values, |value: f32| f64::from(value)),
        VoxelData::F64(values) => arm!(values, |value: f64| value),
    }
}

pub(crate) fn cover(extent: Extent, ratio: u32, width: usize, counts: &mut [u32]) {
    let source_width = extent.x() as usize;
    let ratio = ratio as usize;
    for y in 0..extent.y() as usize {
        let line = (y / ratio) * width;
        for x in 0..width {
            let start = (x * ratio).min(source_width);
            let end = (start + ratio).min(source_width);
            counts[line + x] += (end - start) as u32;
        }
    }
}

pub(crate) fn finish(format: VoxelFormat, sums: &[f64], counts: &[u32]) -> VoxelData {
    assert!(
        counts.iter().all(|count| *count > 0),
        "every resolved voxel averages a stored voxel"
    );
    let mut index = 0;
    VoxelData::from_values(format, sums.len(), || {
        let value = sums[index] / f64::from(counts[index]);
        index += 1;
        value
    })
}

fn blocks<T: Copy>(
    values: &[T],
    extent: Extent,
    ratio: u32,
    width: usize,
    cast: fn(T) -> f64,
    mut add: impl FnMut(usize, f64),
) {
    let source_width = extent.x() as usize;
    let ratio = ratio as usize;
    assert!(
        values.len() >= source_width * extent.y() as usize,
        "the source plane spans its extent"
    );
    for y in 0..extent.y() as usize {
        let row = &values[y * source_width..(y + 1) * source_width];
        let line = (y / ratio) * width;
        for x in 0..width {
            let start = (x * ratio).min(source_width);
            let end = (start + ratio).min(source_width);
            let mut total = 0.0;
            for value in &row[start..end] {
                total += cast(*value);
            }
            add(line + x, total);
        }
    }
}
