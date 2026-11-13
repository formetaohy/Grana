use std::ops::Range;

use grana_object::CellParams;
use grana_volume::{Level, Pyramid, Region};

use super::ridge;

const BAND_BYTES: u64 = 512 << 20;
const BAND_CEILING: u64 = 2 << 30;
const VOXEL_BYTES: u64 = 16;

pub(crate) fn pad(level: Level, reach: f32) -> u32 {
    let step = level.voxel()[2];
    assert!(
        step.is_finite() && step > 0.0,
        "a cell growth level spans a positive z spacing"
    );
    ((2.0 * reach / step).ceil() as u32)
        .saturating_add(1)
        .max(1)
}

pub(crate) fn halo(level: Level, grain: f32) -> u32 {
    ridge::halo(level, grain)
}

pub(crate) fn preferred(
    ranges: &[Range<u32>; 3],
    level: Level,
    params: CellParams,
    reach: f32,
) -> u32 {
    let [range_x, range_y, range_z] = ranges;
    let width = range_x.end - range_x.start;
    let height = range_y.end - range_y.start;
    let depth = range_z.end - range_z.start;
    assert!(
        width > 0 && height > 0 && depth > 0,
        "a cell growth spans a positive box"
    );
    let plane = u64::from(width) * u64::from(height);
    let overhead = 2 * (pad(level, reach).saturating_add(halo(level, params.grain())));
    let planes = (BAND_BYTES / plane.saturating_mul(VOXEL_BYTES)).max(1) as u32;
    let mut band = planes
        .saturating_sub(overhead)
        .max(pad(level, reach).saturating_add(halo(level, params.grain())))
        .max(1)
        .min(depth);
    if band.saturating_add(overhead) >= depth {
        band = depth;
    }
    band
}

pub(crate) fn level(
    pyramid: &Pyramid,
    region: Region,
    params: CellParams,
    radius: f32,
) -> Option<usize> {
    let reach = params.reach() * radius;
    if !(reach.is_finite() && reach > 0.0) {
        return None;
    }
    for index in 0..pyramid.level_count() {
        let info = pyramid.level(index);
        let Some(ranges) = region.voxel_range(info) else {
            continue;
        };
        if ranges.iter().any(Range::is_empty) {
            continue;
        }
        if working_bytes(&ranges, info, params, reach) <= BAND_CEILING {
            return Some(index);
        }
    }
    None
}

pub(crate) fn count(ranges: &[Range<u32>; 3], band: u32) -> usize {
    let depth = ranges[2].end - ranges[2].start;
    depth.div_ceil(band) as usize
}

pub(crate) fn core(ranges: &[Range<u32>; 3], band: u32, index: usize) -> Range<u32> {
    let start = ranges[2].start + band * index as u32;
    let end = start.saturating_add(band).min(ranges[2].end);
    assert!(
        start < end,
        "a cell growth band names planes inside the region"
    );
    start..end
}

pub(crate) fn expanded(range: &Range<u32>, core: Range<u32>, pad: u32) -> Range<u32> {
    let start = core.start.saturating_sub(pad).max(range.start);
    let end = core.end.saturating_add(pad).min(range.end);
    assert!(start < end, "a cell growth band spans one plane");
    start..end
}

fn working_bytes(ranges: &[Range<u32>; 3], level: Level, params: CellParams, reach: f32) -> u64 {
    let width = ranges[0].end - ranges[0].start;
    let height = ranges[1].end - ranges[1].start;
    let depth = ranges[2].end - ranges[2].start;
    let plane = u64::from(width) * u64::from(height);
    let overhead = 2 * (pad(level, reach).saturating_add(halo(level, params.grain())));
    let planes = preferred(ranges, level, params, reach)
        .saturating_add(overhead)
        .min(depth);
    plane
        .saturating_mul(u64::from(planes))
        .saturating_mul(VOXEL_BYTES)
}
