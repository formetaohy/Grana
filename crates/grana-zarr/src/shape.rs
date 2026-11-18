use grana_volume::{Extent, Level};

const INNER: [u32; 3] = [64, 64, 16];
const SUBCHUNKS: [u32; 3] = [4, 4, 2];

pub(crate) fn of(extent: Extent) -> [u32; 3] {
    [extent.x(), extent.y(), extent.z()]
}

pub(crate) fn extent(level: Level) -> [u32; 3] {
    of(level.extent())
}

pub(crate) fn inner(extent: [u32; 3]) -> [u32; 3] {
    std::array::from_fn(|axis| extent[axis].min(INNER[axis]))
}

pub(crate) fn shard(extent: [u32; 3]) -> [u32; 3] {
    let inner = inner(extent);
    std::array::from_fn(|axis| {
        let subchunks = extent[axis].div_ceil(inner[axis]).min(SUBCHUNKS[axis]);
        inner[axis] * subchunks
    })
}

pub(crate) fn count(extent: [u32; 3]) -> u64 {
    let shard = shard(extent);
    (0..3)
        .map(|axis| u64::from(extent[axis].div_ceil(shard[axis])))
        .product()
}

pub(crate) fn voxels(extent: [u32; 3]) -> u64 {
    extent.iter().map(|axis| u64::from(*axis)).product()
}
