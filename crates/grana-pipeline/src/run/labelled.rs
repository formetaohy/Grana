use std::ops::Range;

use grana_analysis::{LabelBuilder, LabelSlab};
use grana_object::SurfaceSet;
use grana_scene::Drifts;
use grana_volume::{Level, NoData, VoxelData};

use crate::channels::Channels;

const LABEL_SLAB: u32 = 32;

pub(super) struct Surfaces {
    builder: LabelBuilder,
}

impl Surfaces {
    pub(super) fn new(level: usize, info: Level, no_data: &[Option<NoData>]) -> Self {
        Self {
            builder: LabelBuilder::new(level, info, no_data),
        }
    }

    pub(super) fn push(
        &mut self,
        origin: [u32; 3],
        core: [u32; 3],
        labels: &[u32],
        channels: &[VoxelData],
        truncated: &[u32],
    ) {
        let plane = core[0] as usize * core[1] as usize;
        assert!(
            plane > 0 && labels.len().is_multiple_of(plane),
            "a label slab covers every voxel of its plane"
        );
        let span = (labels.len() / plane) as u32;
        assert!(
            span >= core[2],
            "a label slab holds its core and the plane above it"
        );
        self.builder.push(LabelSlab {
            origin,
            core,
            extent: [core[0], core[1], span],
            labels,
            channels,
            truncated,
        });
    }

    pub(super) fn finish(self, ids: &grana_object::ObjectIdSource) -> Option<SurfaceSet> {
        self.builder.finish(ids)
    }
}

pub(super) struct Labelled<'a> {
    pub(super) reader: &'a Channels,
    pub(super) time: u32,
    pub(super) level: usize,
    pub(super) ranges: &'a [Range<u32>; 3],
    pub(super) labels: &'a [u32],
    pub(super) truncated: &'a [u32],
    pub(super) drifts: &'a Drifts,
    pub(super) ids: &'a grana_object::ObjectIdSource,
}

pub(super) fn surfaces(labelled: Labelled<'_>) -> Result<Option<SurfaceSet>, String> {
    let Labelled {
        reader,
        time,
        level,
        ranges,
        labels,
        truncated,
        drifts,
        ids,
    } = labelled;
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
    let info = reader.pyramid().level(level);
    let mut builder = Surfaces::new(level, info, &reader.no_data());
    let mut z = 0u32;
    while z < depth {
        let core = LABEL_SLAB.min(depth - z);
        let span = (core + 1).min(depth - z);
        let start = z as usize * plane;
        let slab = &labels[start..start + span as usize * plane];
        let extent = [
            ranges[0].clone(),
            ranges[1].clone(),
            ranges[2].start + z..ranges[2].start + z + span,
        ];
        let mut data = Vec::with_capacity(channels);
        for channel in 0..channels {
            data.push(
                reader
                    .read_region(time, level, channel, &extent, drifts.warp(channel, time))
                    .map_err(|error| error.to_string())?,
            );
        }
        builder.push(
            [ranges[0].start, ranges[1].start, ranges[2].start + z],
            [width, height, core],
            slab,
            &data,
            truncated,
        );
        z += core;
    }
    Ok(builder.finish(ids))
}
