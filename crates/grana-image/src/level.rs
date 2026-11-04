use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use grana_volume::{
    BRICK_SIDE, BrickCoord, ChannelInfo, Extent, Level, Pyramid, VoxelData, VoxelFormat,
};

const RESOLVED_LIMIT: usize = 8;
const CACHE_BYTES: u64 = 256 << 20;
const CHUNK: u32 = 64;

pub(crate) fn extended(
    levels: &[Level],
    channels: &[ChannelInfo],
    frames: u32,
    interval: f32,
) -> Pyramid {
    let mut levels = levels.to_vec();
    let stored = levels.len();
    while levels.len() - stored < RESOLVED_LIMIT {
        let last = *levels.last().expect("a source holds at least one level");
        if last.extent().max_dimension() <= BRICK_SIDE {
            break;
        }
        levels.push(last.coarser());
    }
    Pyramid::new(levels, stored, channels.to_vec(), frames, interval)
}

pub(crate) struct Scale {
    source: usize,
    shift: u32,
    extent: Extent,
    source_extent: Extent,
}

impl Scale {
    pub(crate) fn of(pyramid: &Pyramid, level: usize) -> Self {
        let stored = pyramid.stored();
        assert!(level >= stored, "only resolved levels carry a scale");
        Self {
            source: stored - 1,
            shift: u32::try_from(level - stored + 1)
                .expect("the pyramid holds fewer than 4G levels"),
            extent: pyramid.level(level).extent(),
            source_extent: pyramid.level(stored - 1).extent(),
        }
    }

    pub(crate) fn source(&self) -> usize {
        self.source
    }

    pub(crate) fn shift(&self) -> u32 {
        self.shift
    }

    pub(crate) fn source_ranges(&self, ranges: &[Range<u32>; 3]) -> [Range<u32>; 3] {
        let ratio = 1u32 << self.shift;
        [
            clamped(&ranges[0], ratio, self.source_extent.x()),
            clamped(&ranges[1], ratio, self.source_extent.y()),
            clamped(&ranges[2], ratio, self.source_extent.z()),
        ]
    }

    pub(crate) fn base(&self, ranges: &[Range<u32>; 3]) -> [u32; 3] {
        [
            ranges[0].start << self.shift,
            ranges[1].start << self.shift,
            ranges[2].start << self.shift,
        ]
    }

    pub(crate) fn target(&self, ranges: &[Range<u32>; 3]) -> Extent {
        Extent::new(
            ranges[0].end - ranges[0].start,
            ranges[1].end - ranges[1].start,
            ranges[2].end - ranges[2].start,
        )
    }

    pub(crate) fn within(&self, ranges: &[Range<u32>; 3]) -> bool {
        ranges[0].start < ranges[0].end
            && ranges[0].end <= self.extent.x()
            && ranges[1].start < ranges[1].end
            && ranges[1].end <= self.extent.y()
            && ranges[2].start < ranges[2].end
            && ranges[2].end <= self.extent.z()
    }
}

fn clamped(range: &Range<u32>, ratio: u32, limit: u32) -> Range<u32> {
    let start = (range.start * ratio).min(limit);
    let end = (range.end * ratio).min(limit);
    start..end.max(start)
}

pub(crate) fn chunks<E>(
    ranges: &[Range<u32>; 3],
    mut visit: impl FnMut([Range<u32>; 3]) -> Result<(), E>,
) -> Result<(), E> {
    let mut z = ranges[2].start;
    while z < ranges[2].end {
        let z_end = (z + CHUNK).min(ranges[2].end);
        let mut y = ranges[1].start;
        while y < ranges[1].end {
            let y_end = (y + CHUNK).min(ranges[1].end);
            let mut x = ranges[0].start;
            while x < ranges[0].end {
                let x_end = (x + CHUNK).min(ranges[0].end);
                visit([x..x_end, y..y_end, z..z_end])?;
                x = x_end;
            }
            y = y_end;
        }
        z = z_end;
    }
    Ok(())
}

pub(crate) struct Field {
    extent: Extent,
    sums: Vec<f64>,
    counts: Vec<u32>,
}

impl Field {
    pub(crate) fn new(extent: Extent) -> Self {
        let count = extent.voxel_count() as usize;
        Self {
            extent,
            sums: vec![0.0; count],
            counts: vec![0; count],
        }
    }

    pub(crate) fn add(&mut self, data: &VoxelData, chunk: Extent, offset: [u32; 3], shift: u32) {
        assert_eq!(
            data.voxel_count() as u64,
            chunk.voxel_count(),
            "the chunk matches its extent"
        );
        match data {
            VoxelData::U8(values) => self.fold(values, chunk, offset, shift, f64::from),
            VoxelData::U16(values) => self.fold(values, chunk, offset, shift, f64::from),
            VoxelData::F32(values) => self.fold(values, chunk, offset, shift, f64::from),
        }
    }

    fn fold<T: Copy>(
        &mut self,
        values: &[T],
        chunk: Extent,
        offset: [u32; 3],
        shift: u32,
        cast: fn(T) -> f64,
    ) {
        assert!(shift > 0, "a resolved level halves the stored level");
        let width = chunk.x() as usize;
        let plane = width * chunk.y() as usize;
        for z in 0..chunk.z() as usize {
            let row = ((offset[2] as usize + z) >> shift) as u32;
            for y in 0..chunk.y() as usize {
                let column = ((offset[1] as usize + y) >> shift) as u32;
                let base = self.extent.index(0, column, row) as usize;
                for x in 0..width {
                    let index = base + ((offset[0] as usize + x) >> shift);
                    self.sums[index] += cast(values[z * plane + y * width + x]);
                    self.counts[index] += 1;
                }
            }
        }
    }

    pub(crate) fn finish(self, format: VoxelFormat) -> VoxelData {
        let mean = self
            .sums
            .iter()
            .zip(&self.counts)
            .map(|(sum, count)| {
                assert!(*count > 0, "every resolved voxel averages a stored voxel");
                sum / f64::from(*count)
            })
            .collect::<Vec<_>>();
        match format {
            VoxelFormat::U8 => VoxelData::U8(
                mean.into_iter()
                    .map(|value| value.round().clamp(0.0, f64::from(u8::MAX)) as u8)
                    .collect(),
            ),
            VoxelFormat::U16 => VoxelData::U16(
                mean.into_iter()
                    .map(|value| value.round().clamp(0.0, f64::from(u16::MAX)) as u16)
                    .collect(),
            ),
            VoxelFormat::F32 => {
                VoxelData::F32(mean.into_iter().map(|value| value as f32).collect())
            }
        }
    }
}

pub(crate) type Key = (u32, usize, BrickCoord);

struct Entry {
    bricks: Arc<Vec<VoxelData>>,
    stamp: u64,
}

pub(crate) struct Cache {
    entries: HashMap<Key, Entry>,
    bytes: u64,
    stamp: u64,
}

impl Cache {
    pub(crate) fn new() -> Self {
        Self {
            entries: HashMap::new(),
            bytes: 0,
            stamp: 0,
        }
    }

    pub(crate) fn fetch(&mut self, key: Key) -> Option<Arc<Vec<VoxelData>>> {
        self.stamp += 1;
        let stamp = self.stamp;
        let entry = self.entries.get_mut(&key)?;
        entry.stamp = stamp;
        Some(Arc::clone(&entry.bricks))
    }

    pub(crate) fn insert(&mut self, key: Key, bricks: Arc<Vec<VoxelData>>) {
        self.stamp += 1;
        let bytes = measure(&bricks);
        let previous = self.entries.insert(
            key,
            Entry {
                bricks,
                stamp: self.stamp,
            },
        );
        if let Some(previous) = previous {
            let freed = measure(&previous.bricks);
            self.bytes -= freed;
        }
        self.bytes += bytes;
        while self.bytes > CACHE_BYTES && self.entries.len() > 1 {
            let oldest = *self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.stamp)
                .map(|(key, _)| key)
                .expect("the cache holds an entry to evict");
            if let Some(removed) = self.entries.remove(&oldest) {
                self.bytes -= measure(&removed.bricks);
            }
        }
    }
}

fn measure(bricks: &[VoxelData]) -> u64 {
    bricks
        .iter()
        .map(|channel| {
            let sample = match channel.format() {
                VoxelFormat::U8 => 1,
                VoxelFormat::U16 => 2,
                VoxelFormat::F32 => 4,
            };
            channel.voxel_count() as u64 * sample
        })
        .sum()
}
