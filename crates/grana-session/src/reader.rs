use std::ops::Range;
use std::sync::Arc;

use grana_volume::{BrickCoord, BrickData, BrickGrid, Pyramid, VolumeSeries, VoxelData};
use grana_zarr::Image;

use crate::Source;

pub(crate) struct Reader {
    kind: Kind,
}

enum Kind {
    Memory { series: Arc<VolumeSeries> },
    Zarr(Image),
}

impl Reader {
    pub fn open(source: &Source) -> Result<Self, String> {
        match source {
            Source::Memory(series) => Ok(Self {
                kind: Kind::Memory {
                    series: Arc::clone(series),
                },
            }),
            Source::File(path) => Image::open(path)
                .map(|image| Self {
                    kind: Kind::Zarr(image),
                })
                .map_err(|error| error.to_string()),
        }
    }

    pub fn name(&self) -> &str {
        match &self.kind {
            Kind::Memory { .. } => "synthetic",
            Kind::Zarr(image) => image.name(),
        }
    }

    pub fn pyramid(&self) -> &Pyramid {
        match &self.kind {
            Kind::Memory { series } => series.pyramid(),
            Kind::Zarr(image) => image.pyramid(),
        }
    }

    pub fn region(
        &self,
        time: u32,
        level: usize,
        channel: usize,
        ranges: &[Range<u32>; 3],
    ) -> Result<VoxelData, String> {
        match &self.kind {
            Kind::Memory { series } => Ok(series.region(
                time,
                channel,
                ranges[0].clone(),
                ranges[1].clone(),
                ranges[2].clone(),
            )),
            Kind::Zarr(image) => image
                .read_region(
                    time,
                    level,
                    channel,
                    ranges[0].clone(),
                    ranges[1].clone(),
                    ranges[2].clone(),
                )
                .map_err(|error| error.to_string()),
        }
    }

    pub fn brick(&self, time: u32, level: usize, coord: BrickCoord) -> Result<BrickData, String> {
        match &self.kind {
            Kind::Memory { series } => {
                let grid = BrickGrid::new(series.pyramid().level(level).extent());
                Ok(series.brick(time, &grid, coord))
            }
            Kind::Zarr(image) => image
                .read_brick(time, level, coord)
                .map_err(|error| error.to_string()),
        }
    }
}
