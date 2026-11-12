use std::ops::Range;

use grana_analysis::{LabelBuilder, LabelSlab};
use grana_object::SurfaceSet;
use grana_volume::Level;
use grana_warp::Warp;

use crate::channels::Channels;

const LABEL_SLAB: u32 = 32;

pub(super) fn surfaces(
    reader: &Channels,
    time: u32,
    level: usize,
    info: Level,
    ranges: &[Range<u32>; 3],
    labels: &[u32],
    warp: &Warp,
) -> Result<Option<SurfaceSet>, String> {
    let width = ranges[0].end - ranges[0].start;
    let height = ranges[1].end - ranges[1].start;
    let depth = ranges[2].end - ranges[2].start;
    let plane = width as usize * height as usize;
    assert_eq!(
        labels.len(),
        plane * depth as usize,
        "the labels cover every voxel of their box"
    );
    let channels = reader.pyramid().channel_count();
    let mut builder = LabelBuilder::new(level, info, channels);
    let mut z = 0u32;
    while z < depth {
        let core = LABEL_SLAB.min(depth - z);
        let span = (core + 1).min(depth - z);
        let start = z as usize * plane;
        let slab = &labels[start..start + span as usize * plane];
        let origin = [ranges[0].start, ranges[1].start, ranges[2].start + z];
        let extent = [
            ranges[0].clone(),
            ranges[1].clone(),
            origin[2]..origin[2] + span,
        ];
        let mut data = Vec::with_capacity(channels);
        for channel in 0..channels {
            data.push(
                reader
                    .read_region(time, level, channel, &extent, warp)
                    .map_err(|error| error.to_string())?,
            );
        }
        builder.push(LabelSlab {
            origin,
            core: [width, height, core],
            extent: [width, height, span],
            labels: slab,
            channels: &data,
        });
        z += core;
    }
    Ok(builder.finish())
}
