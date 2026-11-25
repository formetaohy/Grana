use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use grana_store::{LruCache, Pool};
use grana_volume::{Extent, NoData, VoxelData};
use zarrs::array::{Array, ArraySubset};
use zarrs::storage::ReadableStorageTraits;

use crate::Error;
use crate::axes::Layout;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Decode {
    decoded: u64,
    requested: u64,
}

impl Decode {
    pub fn decoded_bytes(self) -> u64 {
        self.decoded
    }

    pub fn requested_bytes(self) -> u64 {
        self.requested
    }

    pub fn amplification(self) -> f32 {
        if self.requested == 0 {
            0.0
        } else {
            self.decoded as f32 / self.requested as f32
        }
    }

    pub(crate) fn merged(self, other: Self) -> Self {
        Self {
            decoded: self.decoded + other.decoded,
            requested: self.requested + other.requested,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Key {
    level: u32,
    time: u32,
    channel: u32,
    coord: [u32; 3],
}

struct Block {
    data: VoxelData,
    extent: Extent,
}

impl Block {
    fn side(&self, axis: usize) -> u32 {
        [self.extent.x(), self.extent.y(), self.extent.z()][axis]
    }
}

static LIVE: AtomicUsize = AtomicUsize::new(0);

pub(crate) struct Tiles {
    blocks: LruCache<Key, Block>,
    decoded: AtomicU64,
    requested: AtomicU64,
}

pub(crate) struct Unit<'a> {
    pub(crate) level: usize,
    pub(crate) array: &'a Array<dyn ReadableStorageTraits>,
    pub(crate) layout: &'a Layout,
    pub(crate) path: &'a str,
    pub(crate) shape: [u32; 3],
    pub(crate) no_data: NoData,
}

impl Drop for Tiles {
    fn drop(&mut self) {
        LIVE.fetch_sub(1, Ordering::Relaxed);
    }
}

impl Tiles {
    pub(crate) fn new() -> Self {
        LIVE.fetch_add(1, Ordering::Relaxed);
        Self {
            blocks: LruCache::scoped(Pool::Decode, &LIVE, |block: &Block| block.data.bytes()),
            decoded: AtomicU64::new(0),
            requested: AtomicU64::new(0),
        }
    }

    pub(crate) fn stats(&self) -> Decode {
        Decode {
            decoded: self.decoded.load(Ordering::Relaxed),
            requested: self.requested.load(Ordering::Relaxed),
        }
    }

    pub(crate) fn read(
        &self,
        unit: &Unit<'_>,
        time: u32,
        channel: u32,
        ranges: &[Range<u32>; 3],
    ) -> Result<VoxelData, Error> {
        let format = crate::image::supported_format(unit.path, unit.array)?;
        let extents = [0, 1, 2].map(|axis| ranges[axis].end - ranges[axis].start);
        let voxels = extents
            .iter()
            .map(|length| u64::from(*length))
            .product::<u64>();
        self.requested
            .fetch_add(voxels * format.bytes() as u64, Ordering::Relaxed);
        let mut output = VoxelData::filled(unit.no_data, voxels as usize);
        let cache = &self.blocks;
        let shape = unit.shape;
        let cacheable = unit_voxels(shape) * format.bytes() as u64 <= cache.share();
        let first = [0, 1, 2].map(|axis| ranges[axis].start / shape[axis]);
        let last = [0, 1, 2].map(|axis| (ranges[axis].end - 1) / shape[axis]);
        for z in first[2]..=last[2] {
            for y in first[1]..=last[1] {
                for x in first[0]..=last[0] {
                    let coord = [x, y, z];
                    let origin = [x * shape[0], y * shape[1], z * shape[2]];
                    let key = Key {
                        level: unit.level as u32,
                        time,
                        channel,
                        coord,
                    };
                    let block = if cacheable {
                        cache.load(key, || self.block(unit, time, channel, origin))?
                    } else {
                        Arc::new(self.block(unit, time, channel, origin)?)
                    };
                    copy(&block, origin, &mut output, ranges);
                }
            }
        }
        Ok(output)
    }

    fn block(
        &self,
        unit: &Unit<'_>,
        time: u32,
        channel: u32,
        origin: [u32; 3],
    ) -> Result<Block, Error> {
        let data = self.decode(unit, time, channel, origin)?;
        let layout = unit.layout;
        let shape = unit.shape;
        Ok(Block {
            data,
            extent: Extent::new(
                shape[0].min(layout.extent.x() - origin[0]),
                shape[1].min(layout.extent.y() - origin[1]),
                shape[2].min(layout.extent.z() - origin[2]),
            ),
        })
    }

    fn decode(
        &self,
        unit: &Unit<'_>,
        time: u32,
        channel: u32,
        origin: [u32; 3],
    ) -> Result<VoxelData, Error> {
        let layout = unit.layout;
        let limits = [layout.extent.x(), layout.extent.y(), layout.extent.z()];
        let ranges = [0, 1, 2].map(|axis| {
            let start = origin[axis];
            start..(origin[axis] + unit.shape[axis]).min(limits[axis])
        });
        let ranges = layout.ranges(
            u64::from(time),
            u64::from(channel),
            ranges[0].clone(),
            ranges[1].clone(),
            ranges[2].clone(),
        );
        let subset = ArraySubset::new_with_ranges(&ranges);
        let data = crate::image::retrieve(unit.array, &subset, unit.path)?;
        self.decoded.fetch_add(data.bytes(), Ordering::Relaxed);
        Ok(data)
    }
}

fn copy(block: &Block, origin: [u32; 3], output: &mut VoxelData, ranges: &[Range<u32>; 3]) {
    let mut source_origin = [0usize; 3];
    let mut target_origin = [0usize; 3];
    let mut length = [0usize; 3];
    for axis in 0..3 {
        let start = origin[axis].max(ranges[axis].start);
        let end = (origin[axis] + block.side(axis)).min(ranges[axis].end);
        if start >= end {
            return;
        }
        source_origin[axis] = (start - origin[axis]) as usize;
        target_origin[axis] = (start - ranges[axis].start) as usize;
        length[axis] = (end - start) as usize;
    }
    let source_extent = [
        block.extent.x() as usize,
        block.extent.y() as usize,
        block.extent.z() as usize,
    ];
    let target_extent = [
        (ranges[0].end - ranges[0].start) as usize,
        (ranges[1].end - ranges[1].start) as usize,
        (ranges[2].end - ranges[2].start) as usize,
    ];
    copy_box(
        &block.data,
        source_extent,
        source_origin,
        output,
        target_extent,
        target_origin,
        length,
    );
}

fn copy_box(
    source: &VoxelData,
    source_extent: [usize; 3],
    source_origin: [usize; 3],
    target: &mut VoxelData,
    target_extent: [usize; 3],
    target_origin: [usize; 3],
    length: [usize; 3],
) {
    macro_rules! boxed {
        ($variant:ident) => {
            if let (VoxelData::$variant(source), VoxelData::$variant(target)) =
                (source, &mut *target)
            {
                rows(
                    source,
                    source_extent,
                    source_origin,
                    target,
                    target_extent,
                    target_origin,
                    length,
                );
            }
        };
    }
    boxed!(U8);
    boxed!(U16);
    boxed!(U32);
    boxed!(U64);
    boxed!(I8);
    boxed!(I16);
    boxed!(I32);
    boxed!(I64);
    boxed!(F32);
    boxed!(F64);
    assert_eq!(
        source.format(),
        target.format(),
        "a voxel copy keeps its format"
    );
}

fn rows<T: Copy>(
    source: &[T],
    source_extent: [usize; 3],
    source_origin: [usize; 3],
    target: &mut [T],
    target_extent: [usize; 3],
    target_origin: [usize; 3],
    length: [usize; 3],
) {
    for z in 0..length[2] {
        for y in 0..length[1] {
            let at = source_origin[0]
                + source_extent[0]
                    * (source_origin[1] + y + source_extent[1] * (source_origin[2] + z));
            let to = target_origin[0]
                + target_extent[0]
                    * (target_origin[1] + y + target_extent[1] * (target_origin[2] + z));
            target[to..to + length[0]].copy_from_slice(&source[at..at + length[0]]);
        }
    }
}

fn unit_voxels(unit: [u32; 3]) -> u64 {
    unit.iter().map(|side| u64::from(*side)).product()
}
