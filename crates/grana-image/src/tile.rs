use std::ops::Range;

use grana_volume::{Extent, VoxelData, VoxelFormat};

pub(crate) const SIDE: u32 = 64;

pub(crate) struct Grid {
    dims: [u32; 3],
    counts: [u32; 3],
}

impl Grid {
    pub(crate) fn of(extent: Extent) -> Self {
        let dims = [extent.x(), extent.y(), extent.z()];
        Self {
            dims,
            counts: dims.map(|dimension| dimension.div_ceil(SIDE)),
        }
    }

    pub(crate) fn range(&self, coord: [u32; 3]) -> [Range<u32>; 3] {
        [0, 1, 2].map(|axis| {
            assert!(
                coord[axis] < self.counts[axis],
                "the tile lies inside the resolution level"
            );
            let start = coord[axis] * SIDE;
            start..(start + SIDE).min(self.dims[axis])
        })
    }

    pub(crate) fn extent_of(&self, coord: [u32; 3]) -> Extent {
        let ranges = self.range(coord);
        Extent::new(
            ranges[0].end - ranges[0].start,
            ranges[1].end - ranges[1].start,
            ranges[2].end - ranges[2].start,
        )
    }

    pub(crate) fn cover(&self, ranges: &[Range<u32>; 3]) -> ([u32; 3], [u32; 3]) {
        let first = [0, 1, 2].map(|axis| ranges[axis].start / SIDE);
        let last = [0, 1, 2].map(|axis| (ranges[axis].end - 1) / SIDE);
        (first, last)
    }
}

pub(crate) fn allocate(format: VoxelFormat, ranges: &[Range<u32>; 3]) -> VoxelData {
    let count = ranges
        .iter()
        .map(|range| u64::from(range.end - range.start))
        .product::<u64>() as usize;
    match format {
        VoxelFormat::U8 => VoxelData::U8(vec![0; count]),
        VoxelFormat::U16 => VoxelData::U16(vec![0; count]),
        VoxelFormat::F32 => VoxelData::F32(vec![0.0; count]),
    }
}

pub(crate) fn copy_intersection(
    out: &mut VoxelData,
    out_ranges: &[Range<u32>; 3],
    source: &VoxelData,
    source_ranges: &[Range<u32>; 3],
) {
    let overlap = [0, 1, 2].map(|axis| {
        out_ranges[axis].start.max(source_ranges[axis].start)
            ..out_ranges[axis].end.min(source_ranges[axis].end)
    });
    if overlap.iter().any(|range| range.start >= range.end) {
        return;
    }
    let out_origin = origin(out_ranges);
    let out_dims = dims(out_ranges);
    let source_origin = origin(source_ranges);
    let source_dims = dims(source_ranges);
    match (out, source) {
        (VoxelData::U8(out), VoxelData::U8(source)) => copy(
            out,
            out_dims,
            out_origin,
            source,
            source_dims,
            source_origin,
            &overlap,
        ),
        (VoxelData::U16(out), VoxelData::U16(source)) => copy(
            out,
            out_dims,
            out_origin,
            source,
            source_dims,
            source_origin,
            &overlap,
        ),
        (VoxelData::F32(out), VoxelData::F32(source)) => copy(
            out,
            out_dims,
            out_origin,
            source,
            source_dims,
            source_origin,
            &overlap,
        ),
        _ => panic!("a tile carries the voxel format of its resolution level"),
    }
}

fn origin(ranges: &[Range<u32>; 3]) -> [u32; 3] {
    [ranges[0].start, ranges[1].start, ranges[2].start]
}

fn dims(ranges: &[Range<u32>; 3]) -> [u32; 3] {
    [
        ranges[0].end - ranges[0].start,
        ranges[1].end - ranges[1].start,
        ranges[2].end - ranges[2].start,
    ]
}

fn copy<T: Copy>(
    out: &mut [T],
    out_dims: [u32; 3],
    out_origin: [u32; 3],
    source: &[T],
    source_dims: [u32; 3],
    source_origin: [u32; 3],
    overlap: &[Range<u32>; 3],
) {
    let width = (overlap[0].end - overlap[0].start) as usize;
    for z in overlap[2].clone() {
        for y in overlap[1].clone() {
            let source_at = (overlap[0].start - source_origin[0]) as usize
                + source_dims[0] as usize
                    * ((y - source_origin[1]) as usize
                        + source_dims[1] as usize * (z - source_origin[2]) as usize);
            let out_at = (overlap[0].start - out_origin[0]) as usize
                + out_dims[0] as usize
                    * ((y - out_origin[1]) as usize
                        + out_dims[1] as usize * (z - out_origin[2]) as usize);
            out[out_at..out_at + width].copy_from_slice(&source[source_at..source_at + width]);
        }
    }
}
