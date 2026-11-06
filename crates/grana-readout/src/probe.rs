use std::path::Path;
use std::sync::Arc;

use grana_volume::Intensity;
use openreadout_core::{Dataset, ImageInfo, PixelType, Plane, PlaneIndex};

use crate::error::Error;
use crate::plane::{Cache, View};

const STEP: u32 = 4;

pub(crate) fn intensities(
    path: &Path,
    dataset: &mut dyn Dataset,
    info: &ImageInfo,
    views: &[View],
    cache: &mut Cache,
) -> Result<Vec<Option<Intensity>>, Error> {
    let depth = (info.size_z / 2).max(1) - 1;
    let mut ranges = Vec::with_capacity(views.len());
    for view in views {
        let key = (0, view.channel, depth);
        let plane = match cache.get(&key) {
            Some(plane) => plane,
            None => {
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
                    return Err(Error::read(path, "a probed plane holds an unexpected size"));
                }
                if usize::try_from(plane.samples_per_pixel)
                    .map_or(true, |samples| view.sample >= samples)
                {
                    return Err(Error::read(
                        path,
                        "a probed plane lacks the declared samples",
                    ));
                }
                let plane = Arc::new(plane);
                cache.put(key, Arc::clone(&plane), plane.data.len());
                plane
            }
        };
        ranges.push(range(&plane, view.sample));
    }
    Ok(ranges)
}

fn range(plane: &Plane, sample: usize) -> Option<Intensity> {
    let samples = plane.samples_per_pixel as usize;
    let bytes = plane.pixel_type.bytes_per_sample();
    let row_bytes = plane.row_bytes();
    let mut low = f32::INFINITY;
    let mut high = f32::NEG_INFINITY;
    for line in (0..plane.height).step_by(STEP as usize) {
        let start = row_bytes * line as usize;
        for column in (0..plane.width).step_by(STEP as usize) {
            let value = value(plane, start + (column as usize * samples + sample) * bytes);
            if value.is_finite() {
                low = low.min(value);
                high = high.max(value);
            }
        }
    }
    (high > low).then(|| Intensity::new(low, high))
}

fn value(plane: &Plane, at: usize) -> f32 {
    match plane.pixel_type {
        PixelType::Uint8 => f32::from(plane.data[at]),
        PixelType::Uint16 => f32::from(u16::from_le_bytes([plane.data[at], plane.data[at + 1]])),
        PixelType::Float => f32::from_le_bytes([
            plane.data[at],
            plane.data[at + 1],
            plane.data[at + 2],
            plane.data[at + 3],
        ]),
        other => other.sample_f64(&plane.data[at..]) as f32,
    }
}
