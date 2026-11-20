use std::collections::{HashMap, VecDeque};
use std::ops::Range;
use std::sync::{Arc, Mutex, MutexGuard};

use grana_volume::{Extent, VoxelData, VoxelFormat, VoxelSemantics};

const SOURCE_BUDGET: u64 = 512 << 20;
const TOTAL_BUDGET: u64 = 1536 << 20;
const MINIMUM_BUDGET: u64 = 48 << 20;

const RATIO: u32 = 2;

pub(crate) const STRIP_ROWS: usize = 64;

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
    levels: Vec<Mutex<Inner>>,
    budgets: Vec<u64>,
}

struct Inner {
    entries: HashMap<Key, Arc<Plane>>,
    order: VecDeque<Key>,
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
                    Mutex::new(Inner {
                        entries: HashMap::new(),
                        order: VecDeque::new(),
                        bytes: 0,
                    })
                })
                .collect(),
            budgets,
        }
    }

    pub(crate) fn fetch(&self, key: Key) -> Option<Arc<Plane>> {
        let mut inner = self.lock(key.level as usize);
        let plane = inner.entries.get(&key).cloned()?;
        inner.order.retain(|held| *held != key);
        inner.order.push_back(key);
        Some(plane)
    }

    pub(crate) fn insert(&self, key: Key, plane: Arc<Plane>) {
        let budget = self.budgets[key.level as usize];
        let mut inner = self.lock(key.level as usize);
        if let Some(held) = inner.entries.insert(key, plane) {
            inner.bytes = inner.bytes.saturating_sub(held.bytes());
        }
        inner.order.retain(|held| *held != key);
        inner.order.push_back(key);
        inner.bytes += inner.entries[&key].bytes();
        while inner.bytes > budget {
            let Some(victim) = inner.order.pop_front() else {
                break;
            };
            if let Some(held) = inner.entries.remove(&victim) {
                inner.bytes = inner.bytes.saturating_sub(held.bytes());
            }
        }
    }

    fn lock(&self, level: usize) -> MutexGuard<'_, Inner> {
        self.levels[level]
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }
}

pub(crate) fn reduce(
    format: VoxelFormat,
    semantics: VoxelSemantics,
    extent: Extent,
    width: usize,
    rows: Range<usize>,
    sources: &[&VoxelData],
) -> VoxelData {
    assert!(
        !sources.is_empty(),
        "a reduced plane averages at least one source plane"
    );
    if !semantics.averages() {
        return nearest(sources[0], extent, width, rows);
    }
    let height = rows.end - rows.start;
    let voxels = width * height;
    let mut sums = vec![0.0f64; voxels];
    for source in sources {
        add(source, extent, width, rows.clone(), &mut sums);
    }
    let mut counts = vec![0u32; voxels];
    cover(extent, width, rows, &mut counts);
    let depths = u32::try_from(sources.len()).expect("a plane stack counts fewer than 4G planes");
    counts.iter_mut().for_each(|count| *count *= depths);
    finish(format, sums, counts)
}

fn nearest(source: &VoxelData, extent: Extent, width: usize, rows: Range<usize>) -> VoxelData {
    macro_rules! arm {
        ($variant:ident, $values:expr) => {
            VoxelData::$variant(stride($values, extent, width, rows))
        };
    }
    match source {
        VoxelData::U8(values) => arm!(U8, values),
        VoxelData::U16(values) => arm!(U16, values),
        VoxelData::U32(values) => arm!(U32, values),
        VoxelData::U64(values) => arm!(U64, values),
        VoxelData::I8(values) => arm!(I8, values),
        VoxelData::I16(values) => arm!(I16, values),
        VoxelData::I32(values) => arm!(I32, values),
        VoxelData::I64(values) => arm!(I64, values),
        VoxelData::F32(values) => arm!(F32, values),
        VoxelData::F64(values) => arm!(F64, values),
    }
}

fn stride<T: Copy>(values: &[T], extent: Extent, width: usize, rows: Range<usize>) -> Vec<T> {
    let source_width = extent.x() as usize;
    let source_height = extent.y() as usize;
    assert!(
        values.len() >= source_width * source_height,
        "the source plane spans its extent"
    );
    let ratio = RATIO as usize;
    let mut output = Vec::with_capacity(width * (rows.end - rows.start));
    for row in rows {
        let source = (row * ratio).min(source_height - 1) * source_width;
        for column in 0..width {
            output.push(values[source + (column * ratio).min(source_width - 1)]);
        }
    }
    output
}

fn add(source: &VoxelData, extent: Extent, width: usize, rows: Range<usize>, sums: &mut [f64]) {
    macro_rules! arm {
        ($values:expr, $cast:expr) => {
            blocks($values, extent, width, rows, $cast, |index, total| {
                sums[index] += total
            })
        };
    }
    match source {
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

fn cover(extent: Extent, width: usize, rows: Range<usize>, counts: &mut [u32]) {
    let source_width = extent.x() as usize;
    let ratio = RATIO as usize;
    let first = rows.start * ratio;
    let last = (rows.end * ratio).min(extent.y() as usize);
    for y in first..last {
        let line = (y / ratio - rows.start) * width;
        for x in 0..width {
            let start = (x * ratio).min(source_width);
            let end = (start + ratio).min(source_width);
            counts[line + x] += (end - start) as u32;
        }
    }
}

fn finish(format: VoxelFormat, sums: Vec<f64>, counts: Vec<u32>) -> VoxelData {
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
    width: usize,
    rows: Range<usize>,
    cast: fn(T) -> f64,
    mut add: impl FnMut(usize, f64),
) {
    let source_width = extent.x() as usize;
    let ratio = RATIO as usize;
    assert!(
        values.len() >= source_width * extent.y() as usize,
        "the source plane spans its extent"
    );
    let first = rows.start * ratio;
    let last = (rows.end * ratio).min(extent.y() as usize);
    for y in first..last {
        let row = &values[y * source_width..(y + 1) * source_width];
        let line = (y / ratio - rows.start) * width;
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
