use std::ops::Range;

use grana_volume::{VoxelData, VoxelFormat};

pub(crate) fn allocate(format: VoxelFormat, ranges: &[Range<u32>; 3]) -> VoxelData {
    let count = ranges
        .iter()
        .map(|range| u64::from(range.end - range.start))
        .product::<u64>() as usize;
    VoxelData::blank(format, count)
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
    macro_rules! copy_arms {
        ($($variant:ident),*) => {
            match (out, source) {
                $(
                    (VoxelData::$variant(out), VoxelData::$variant(source)) => copy(
                        out,
                        out_dims,
                        out_origin,
                        source,
                        source_dims,
                        source_origin,
                        &overlap,
                    ),
                )*
                _ => panic!("a resolved brick carries the voxel format of its level"),
            }
        };
    }
    copy_arms!(U8, U16, U32, U64, I8, I16, I32, I64, F32, F64)
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

pub(crate) fn copy_rows(
    out: &mut VoxelData,
    out_width: usize,
    out_row: usize,
    source: &VoxelData,
    source_width: usize,
    x: Range<u32>,
    rows: Range<u32>,
) {
    let width = (x.end - x.start) as usize;
    assert!(
        out_row + (rows.end - rows.start) as usize <= out.voxel_count() / out_width.max(1),
        "the copied rows land inside the destination"
    );
    macro_rules! copy_arms {
        ($($variant:ident),*) => {
            match (out, source) {
                $(
                    (VoxelData::$variant(out), VoxelData::$variant(source)) => {
                        for (offset, row) in rows.enumerate() {
                            let from = row as usize * source_width + x.start as usize;
                            let to = (out_row + offset) * out_width;
                            out[to..to + width].copy_from_slice(&source[from..from + width]);
                        }
                    }
                )*
                _ => panic!("a resolved plane carries the voxel format of its level"),
            }
        };
    }
    copy_arms!(U8, U16, U32, U64, I8, I16, I32, I64, F32, F64)
}
