use std::path::Path;
use std::sync::Arc;

use grana_volume::{Extent, Statistics, VoxelData};
use openreadout_core::{Dataset, ImageInfo, PixelType, Plane, PlaneIndex};

use crate::error::Error;
use crate::plane::{self, Key, View};

const STEP: u32 = 4;

pub(crate) fn statistics(
    path: &Path,
    dataset: &mut dyn Dataset,
    info: &ImageInfo,
    views: &[View],
    source: u64,
) -> Result<Vec<Statistics>, Error> {
    let extent = Extent::new(info.size_x.max(1), info.size_y.max(1), info.size_z.max(1));
    let census = grana_volume::Census::of(extent, 1);
    let blocks = census.blocks().to_vec();
    let mut ranges = Vec::with_capacity(views.len());
    for view in views {
        let plane = plane_at(path, dataset, info, view, 0, source)?;
        let mut measured = Statistics::census(&samples(&plane, view.sample), None);
        for block in &blocks {
            let block_extent = Extent::new(
                block[0].end - block[0].start,
                block[1].end - block[1].start,
                block[2].end - block[2].start,
            );
            if block_extent.z() > 1 {
                continue;
            }
            let strided = strided(&plane, view.sample, block);
            measured = measured.folded(Statistics::census(&strided, None));
        }
        ranges.push(measured);
    }
    Ok(ranges)
}

fn plane_at(
    path: &Path,
    dataset: &mut dyn Dataset,
    info: &ImageInfo,
    view: &View,
    depth: u32,
    source: u64,
) -> Result<Arc<Plane>, Error> {
    let key = Key {
        source,
        level: 0,
        time: 0,
        channel: view.channel,
        depth,
    };
    plane::shards().fetch(&key, || {
        let plane = dataset
            .read_plane(
                info.index,
                PlaneIndex {
                    c: view.channel,
                    z: depth,
                    t: 0,
                },
            )
            .map_err(|error| Error::read(path, error.to_string()))?;
        if plane.data.len() != plane.expected_len() {
            return Err(Error::read(
                path,
                "a censused plane holds an unexpected size",
            ));
        }
        if usize::try_from(plane.samples_per_pixel).map_or(true, |samples| view.sample >= samples) {
            return Err(Error::read(
                path,
                "a censused plane lacks the declared samples",
            ));
        }
        Ok(Arc::new(plane))
    })
}

fn samples(plane: &Plane, sample: usize) -> VoxelData {
    let mut values = Vec::with_capacity(plane.width as usize * plane.height as usize);
    let bytes = plane.pixel_type.bytes_per_sample();
    let row_bytes = plane.row_bytes();
    for line in 0..plane.height as usize {
        let start = row_bytes * line;
        for column in 0..plane.width as usize {
            values.push(decode(
                plane.pixel_type,
                &plane.data,
                start + (column * plane.samples_per_pixel as usize + sample) * bytes,
            ));
        }
    }
    VoxelData::F32(values)
}

fn strided(plane: &Plane, sample: usize, block: &[std::ops::Range<u32>; 3]) -> VoxelData {
    let bytes = plane.pixel_type.bytes_per_sample();
    let row_bytes = plane.row_bytes();
    let mut values = Vec::new();
    let stride = STEP as usize;
    let x = &block[0];
    let y = &block[1];
    for line in (y.start as usize..(y.end as usize).min(plane.height as usize)).step_by(stride) {
        let start = row_bytes * line;
        let end = (x.end as usize).min(plane.width as usize);
        for column in (x.start as usize..end).step_by(stride) {
            values.push(decode(
                plane.pixel_type,
                &plane.data,
                start + (column * plane.samples_per_pixel as usize + sample) * bytes,
            ));
        }
    }
    VoxelData::F32(values)
}

fn decode(pixel: PixelType, data: &[u8], at: usize) -> f32 {
    match pixel {
        PixelType::Uint8 => f32::from(data[at]),
        PixelType::Uint16 => f32::from(u16::from_le_bytes([data[at], data[at + 1]])),
        PixelType::Float => {
            f32::from_le_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]])
        }
        other => other.sample_f64(&data[at..]) as f32,
    }
}
