use std::ops::Range;

use grana_volume::{NoData, VoxelData};

pub(crate) fn allocate(no_data: NoData, ranges: &[Range<u32>; 3]) -> VoxelData {
    let count = ranges
        .iter()
        .map(|range| u64::from(range.end - range.start))
        .product::<u64>() as usize;
    VoxelData::filled(no_data, count)
}

pub(crate) fn copy_plane(
    out: &mut VoxelData,
    out_ranges: &[Range<u32>; 3],
    source: &VoxelData,
    source_dims: [u32; 2],
    columns: [Range<u32>; 2],
    z: u32,
) {
    assert_eq!(
        out.format(),
        source.format(),
        "a plane carries the format of the volume it serves"
    );
    assert_eq!(
        source.voxel_count() as u64,
        u64::from(source_dims[0]) * u64::from(source_dims[1]),
        "a plane holds one channel of a whole slice"
    );
    if z < out_ranges[2].start || z >= out_ranges[2].end {
        return;
    }
    let x = out_ranges[0].start.max(columns[0].start)..out_ranges[0].end.min(columns[0].end);
    let y = out_ranges[1].start.max(columns[1].start)..out_ranges[1].end.min(columns[1].end);
    if x.start >= x.end || y.start >= y.end {
        return;
    }
    let out_dims = dims(out_ranges);
    let out_origin = origin(out_ranges);
    macro_rules! copy_arms {
        ($($variant:ident),*) => {
            match (out, source) {
                $(
                    (VoxelData::$variant(out), VoxelData::$variant(source)) => plane_rows(
                        out,
                        out_dims,
                        out_origin,
                        source,
                        source_dims,
                        &x,
                        &y,
                        z,
                    ),
                )*
                _ => panic!("a plane carries the format of the volume it serves"),
            }
        };
    }
    copy_arms!(U8, U16, U32, U64, I8, I16, I32, I64, F32, F64)
}

#[allow(clippy::too_many_arguments)]
fn plane_rows<T: Copy>(
    out: &mut [T],
    out_dims: [u32; 3],
    out_origin: [u32; 3],
    source: &[T],
    source_dims: [u32; 2],
    x: &Range<u32>,
    y: &Range<u32>,
    z: u32,
) {
    let width = (x.end - x.start) as usize;
    for line in y.clone() {
        let source_at = x.start as usize + source_dims[0] as usize * line as usize;
        let out_at = (x.start - out_origin[0]) as usize
            + out_dims[0] as usize
                * ((line - out_origin[1]) as usize
                    + out_dims[1] as usize * (z - out_origin[2]) as usize);
        out[out_at..out_at + width].copy_from_slice(&source[source_at..source_at + width]);
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
