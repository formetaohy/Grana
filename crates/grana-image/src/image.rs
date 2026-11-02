use std::fs::File;
use std::io::Read;
use std::ops::Range;
use std::path::Path;
use std::sync::Arc;

use grana_volume::{BrickCoord, BrickData, BrickGrid, Pyramid, VolumeSeries, VoxelData};

use crate::{Error, Source};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Zarr,
    Tiff,
    Czi,
}

enum Inner {
    Memory(Arc<VolumeSeries>),
    Zarr(grana_zarr::Image),
    Tiff(grana_tiff::Image),
    Czi(grana_czi::Image),
}

pub struct Image {
    inner: Inner,
}

impl Image {
    pub fn open(source: &Source) -> Result<Self, Error> {
        match source {
            Source::Memory(series) => Ok(Self {
                inner: Inner::Memory(Arc::clone(series)),
            }),
            Source::File(path) => Self::open_file(path),
        }
    }

    fn open_file(path: &Path) -> Result<Self, Error> {
        if path.is_dir() {
            let image = grana_zarr::Image::open(path)?;
            return Ok(Self {
                inner: Inner::Zarr(image),
            });
        }
        if is_czi(path)? {
            let image = grana_czi::Image::open(path)?;
            return Ok(Self {
                inner: Inner::Czi(image),
            });
        }
        if !is_tiff(path)? {
            return Err(Error::Unsupported {
                path: path.to_path_buf(),
                detail: "the file is neither an OME-TIFF nor a CZI image".to_string(),
            });
        }
        let image = grana_tiff::Image::open(path)?;
        Ok(Self {
            inner: Inner::Tiff(image),
        })
    }

    pub fn name(&self) -> &str {
        match &self.inner {
            Inner::Memory(_) => "synthetic",
            Inner::Zarr(image) => image.name(),
            Inner::Tiff(image) => image.name(),
            Inner::Czi(image) => image.name(),
        }
    }

    pub fn pyramid(&self) -> &Pyramid {
        match &self.inner {
            Inner::Memory(series) => series.pyramid(),
            Inner::Zarr(image) => image.pyramid(),
            Inner::Tiff(image) => image.pyramid(),
            Inner::Czi(image) => image.pyramid(),
        }
    }

    pub fn format(&self) -> Option<Format> {
        match &self.inner {
            Inner::Memory(_) => None,
            Inner::Zarr(_) => Some(Format::Zarr),
            Inner::Tiff(_) => Some(Format::Tiff),
            Inner::Czi(_) => Some(Format::Czi),
        }
    }

    pub fn read_region(
        &self,
        time: u32,
        level: usize,
        channel: usize,
        ranges: &[Range<u32>; 3],
    ) -> Result<VoxelData, Error> {
        match &self.inner {
            Inner::Memory(series) => {
                require_base(level)?;
                Ok(series.region(
                    time,
                    channel,
                    ranges[0].clone(),
                    ranges[1].clone(),
                    ranges[2].clone(),
                ))
            }
            Inner::Zarr(image) => Ok(image.read_region(
                time,
                level,
                channel,
                ranges[0].clone(),
                ranges[1].clone(),
                ranges[2].clone(),
            )?),
            Inner::Tiff(image) => Ok(image.read_region(
                time,
                level,
                channel,
                ranges[0].clone(),
                ranges[1].clone(),
                ranges[2].clone(),
            )?),
            Inner::Czi(image) => Ok(image.read_region(
                time,
                level,
                channel,
                ranges[0].clone(),
                ranges[1].clone(),
                ranges[2].clone(),
            )?),
        }
    }

    pub fn read_brick(
        &self,
        time: u32,
        level: usize,
        coord: BrickCoord,
    ) -> Result<BrickData, Error> {
        match &self.inner {
            Inner::Memory(series) => {
                require_base(level)?;
                let grid = BrickGrid::new(series.pyramid().level(level).extent());
                Ok(series.brick(time, &grid, coord))
            }
            Inner::Zarr(image) => Ok(image.read_brick(time, level, coord)?),
            Inner::Tiff(image) => Ok(image.read_brick(time, level, coord)?),
            Inner::Czi(image) => Ok(image.read_brick(time, level, coord)?),
        }
    }
}

fn require_base(level: usize) -> Result<(), Error> {
    if level == 0 {
        return Ok(());
    }
    Err(Error::Memory {
        detail: "an in-memory series carries a single resolution level".to_string(),
    })
}

fn is_tiff(path: &Path) -> Result<bool, Error> {
    magic(path, 4).map(|magic| {
        magic
            .as_deref()
            .is_some_and(|magic| matches!(magic, b"II*\0" | b"MM\0*" | b"II+\0" | b"MM\0+"))
    })
}

fn is_czi(path: &Path) -> Result<bool, Error> {
    magic(path, 10).map(|magic| magic.as_deref().is_some_and(|magic| magic == b"ZISRAWFILE"))
}

fn magic(path: &Path, length: usize) -> Result<Option<Vec<u8>>, Error> {
    let mut file = File::open(path).map_err(|error| Error::Open {
        path: path.to_path_buf(),
        detail: error.to_string(),
    })?;
    let mut bytes = vec![0u8; length];
    if file.read_exact(&mut bytes).is_err() {
        return Ok(None);
    }
    Ok(Some(bytes))
}

fn is_zarr(path: &Path) -> bool {
    [".zarray", ".zgroup", "zarr.json"]
        .iter()
        .any(|name| path.join(name).exists())
}

pub fn is_image(path: &Path) -> bool {
    if path.is_dir() {
        is_zarr(path)
    } else {
        is_czi(path).unwrap_or(false) || is_tiff(path).unwrap_or(false)
    }
}

pub fn dataset_name(path: &Path) -> String {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return "dataset".to_string();
    };
    if let Some(stem) = name.strip_suffix(".zarr") {
        return stem.to_string();
    }
    if name.to_ascii_lowercase().ends_with(".czi") {
        return grana_czi::dataset_name(Path::new(name));
    }
    grana_tiff::dataset_name(Path::new(name))
}
