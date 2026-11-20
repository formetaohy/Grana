use std::path::Path;

use crate::directory::Directory;
use crate::error::Error;
use crate::store::Store;
use crate::tag::{
    TAG_ROWS_PER_STRIP, TAG_STRIP_BYTE_COUNTS, TAG_STRIP_OFFSETS, TAG_TILE_BYTE_COUNTS,
    TAG_TILE_LENGTH, TAG_TILE_OFFSETS, TAG_TILE_WIDTH,
};

#[derive(Clone, Copy, Debug)]
pub(crate) struct Block {
    pub index: usize,
    pub origin: (u32, u32),
    pub width: u32,
    pub rows: u32,
    pub offset: u64,
    pub count: u64,
    pub padding: bool,
}

pub(crate) enum Layout {
    Strips {
        rows: u32,
        per_plane: usize,
        offsets: Vec<u64>,
        counts: Vec<u64>,
    },
    Tiles {
        width: u32,
        height: u32,
        per_plane: usize,
        offsets: Vec<u64>,
        counts: Vec<u64>,
    },
}

impl Layout {
    pub(crate) fn new(
        directory: &Directory,
        store: &Store,
        width: u32,
        height: u32,
        samples: u32,
        planar: bool,
        path: &Path,
    ) -> Result<Self, Error> {
        let tile_width = directory.integer(store, TAG_TILE_WIDTH, path)?;
        let tile_length = directory.integer(store, TAG_TILE_LENGTH, path)?;
        if tile_width.is_some() || tile_length.is_some() {
            let tile_width = block_dimension(tile_width, path, TAG_TILE_WIDTH)?;
            let tile_length = block_dimension(tile_length, path, TAG_TILE_LENGTH)?;
            let per_plane =
                width.div_ceil(tile_width) as usize * height.div_ceil(tile_length) as usize;
            let offsets = directory.integers(store, TAG_TILE_OFFSETS, path)?;
            let counts = directory.integers(store, TAG_TILE_BYTE_COUNTS, path)?;
            check(path, "tile", per_plane, samples, planar, &offsets, &counts)?;
            return Ok(Self::Tiles {
                width: tile_width,
                height: tile_length,
                per_plane,
                offsets,
                counts,
            });
        }
        let rows = directory
            .integer(store, TAG_ROWS_PER_STRIP, path)?
            .map(|rows| block_dimension(Some(rows), path, TAG_ROWS_PER_STRIP))
            .transpose()?
            .unwrap_or(height);
        let per_plane = height.div_ceil(rows) as usize;
        let offsets = directory.integers(store, TAG_STRIP_OFFSETS, path)?;
        let counts = directory.integers(store, TAG_STRIP_BYTE_COUNTS, path)?;
        check(path, "strip", per_plane, samples, planar, &offsets, &counts)?;
        Ok(Self::Strips {
            rows,
            per_plane,
            offsets,
            counts,
        })
    }

    pub(crate) fn per_plane(&self) -> usize {
        match self {
            Self::Strips { per_plane, .. } | Self::Tiles { per_plane, .. } => *per_plane,
        }
    }

    pub(crate) fn block(&self, sample: u32, index: usize, width: u32, height: u32) -> Block {
        let (origin, block_width, rows, padding) = match self {
            Self::Strips { rows, .. } => {
                let origin = (0, index as u32 * *rows);
                let rows = (*rows).min(height - origin.1);
                (origin, width, rows, false)
            }
            Self::Tiles {
                width: tile_width,
                height: tile_length,
                ..
            } => {
                let across = width.div_ceil(*tile_width) as usize;
                let origin = (
                    (index % across) as u32 * tile_width,
                    (index / across) as u32 * tile_length,
                );
                (origin, *tile_width, *tile_length, true)
            }
        };
        let flat = index + sample as usize * self.per_plane();
        let (offset, count) = match self {
            Self::Strips {
                offsets, counts, ..
            }
            | Self::Tiles {
                offsets, counts, ..
            } => (offsets[flat], counts[flat]),
        };
        Block {
            index,
            origin,
            width: block_width,
            rows,
            offset,
            count,
            padding,
        }
    }

    pub(crate) fn covering(
        &self,
        region: &[(u32, u32); 2],
        width: u32,
        height: u32,
        sample: u32,
    ) -> Vec<Block> {
        let mut blocks = Vec::new();
        match self {
            Self::Strips { rows, .. } => {
                let first = region[1].0 / rows;
                let last = (region[1].1 - 1) / rows;
                for strip in first..=last {
                    blocks.push(self.block(sample, strip as usize, width, height));
                }
            }
            Self::Tiles {
                width: tile_width,
                height: tile_length,
                ..
            } => {
                let across = width.div_ceil(*tile_width);
                let first_x = region[0].0 / tile_width;
                let last_x = (region[0].1 - 1) / tile_width;
                let first_y = region[1].0 / tile_length;
                let last_y = (region[1].1 - 1) / tile_length;
                for row in first_y..=last_y {
                    for column in first_x..=last_x {
                        let index = (row * across + column) as usize;
                        blocks.push(self.block(sample, index, width, height));
                    }
                }
            }
        }
        blocks
    }

    pub(crate) fn bytes(&self) -> u64 {
        match self {
            Self::Strips {
                offsets, counts, ..
            }
            | Self::Tiles {
                offsets, counts, ..
            } => {
                ((offsets.capacity() + counts.capacity()) * size_of::<u64>()) as u64
                    + size_of::<Self>() as u64
            }
        }
    }
}

fn block_dimension(value: Option<u64>, path: &Path, tag: u16) -> Result<u32, Error> {
    u32::try_from(
        value.ok_or_else(|| Error::unsupported(path, format!("the TIFF tag {tag} is missing")))?,
    )
    .ok()
    .filter(|value| *value > 0)
    .ok_or_else(|| Error::unsupported(path, format!("the TIFF tag {tag} is out of range")))
}

fn check(
    path: &Path,
    noun: &str,
    per_plane: usize,
    samples: u32,
    planar: bool,
    offsets: &[u64],
    counts: &[u64],
) -> Result<(), Error> {
    let expected = if planar {
        per_plane * samples as usize
    } else {
        per_plane
    };
    if offsets.len() != expected || counts.len() != expected {
        return Err(Error::read(
            path,
            format!(
                "the TIFF page declares {expected} {noun} blocks but lists {} offsets and {} byte counts",
                offsets.len(),
                counts.len()
            ),
        ));
    }
    for (index, (offset, count)) in offsets.iter().zip(counts).enumerate() {
        if *offset == 0 && *count != 0 {
            return Err(Error::read(
                path,
                format!("the TIFF page leaves {noun} {index} without pixel data"),
            ));
        }
    }
    Ok(())
}
