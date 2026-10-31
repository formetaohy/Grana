use std::ops::Range;
use std::sync::Arc;

use grana_volume::{BrickCoord, BrickData, BrickGrid, Movie, Pyramid, VoxelData};
use grana_zarr::Image;

use crate::Source;

pub(crate) struct Reader {
    kind: Kind,
}

enum Kind {
    Memory { movie: Arc<Movie>, grid: BrickGrid },
    Zarr(Image),
}

impl Reader {
    pub fn open(source: &Source) -> Result<Self, String> {
        match source {
            Source::Memory(movie) => {
                let grid = BrickGrid::new(movie.pyramid().level(0).extent());
                Ok(Self {
                    kind: Kind::Memory {
                        movie: Arc::clone(movie),
                        grid,
                    },
                })
            }
            Source::File(path) => Image::open(path)
                .map(|image| Self {
                    kind: Kind::Zarr(image),
                })
                .map_err(|error| error.to_string()),
        }
    }

    pub fn pyramid(&self) -> &Pyramid {
        match &self.kind {
            Kind::Memory { movie, .. } => movie.pyramid(),
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
            Kind::Memory { movie, .. } => Ok(movie.region(
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

    pub fn brick(&self, time: u32, coord: BrickCoord) -> Result<BrickData, String> {
        match &self.kind {
            Kind::Memory { movie, grid } => Ok(movie.brick(time, grid, coord)),
            Kind::Zarr(image) => image
                .read_brick(time, 0, coord)
                .map_err(|error| error.to_string()),
        }
    }
}
