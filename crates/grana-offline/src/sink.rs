use std::path::{Path, PathBuf};

use grana_raster::Raster;
use grana_scene::Scene;
use grana_video::{Recorder, Sequence};

pub(crate) enum Sink {
    Png { path: PathBuf },
    Tiff { path: PathBuf },
    Movie { path: PathBuf, recorder: Recorder },
    Sequence { sequence: Sequence },
}

impl Sink {
    pub(crate) fn push(&mut self, time: u32, size: [u32; 2], pixels: &[u8]) -> Result<(), String> {
        let raster = || Raster::new(size[0], size[1], pixels.to_vec());
        match self {
            Self::Png { path } => grana_raster::write(path, &raster()).map_err(String::from),
            Self::Tiff { .. } => {
                unreachable!("a wide picture is written from the rendered frame")
            }
            Self::Movie { recorder, .. } => {
                recorder.push(pixels).map_err(|error| error.to_string())
            }
            Self::Sequence { sequence } => {
                let png = grana_raster::encode(&raster()).map_err(String::from)?;
                sequence.push(time, &png).map_err(|error| error.to_string())
            }
        }
    }

    pub(crate) fn finish(self) -> Result<PathBuf, String> {
        match self {
            Self::Png { path } => Ok(path),
            Self::Tiff { path } => Ok(path),
            Self::Movie { path, recorder } => {
                recorder.finish().map_err(|error| error.to_string())?;
                Ok(path)
            }
            Self::Sequence { sequence } => sequence.finish().map_err(|error| error.to_string()),
        }
    }

    pub(crate) fn discard(self) {
        match self {
            Self::Png { path } | Self::Tiff { path } => {
                let _ = std::fs::remove_file(path);
            }
            Self::Movie { path, recorder } => {
                drop(recorder);
                let _ = std::fs::remove_file(path);
            }
            Self::Sequence { sequence } => sequence.discard(),
        }
    }
}

pub(crate) fn write_picture(
    path: &Path,
    size: [u32; 2],
    pixels: &[u16],
    scene: &Scene,
    time: u32,
    transparent: bool,
) -> Result<(), String> {
    let name = if scene.dataset.frame_count() > 1 {
        format!("{} · t={time}", scene.dataset.name())
    } else {
        scene.dataset.name().to_string()
    };
    let rgb;
    let picture = if transparent {
        grana_tiff::Picture {
            size,
            samples: 4,
            pixels,
            name: &name,
        }
    } else {
        rgb = strip_alpha(pixels);
        grana_tiff::Picture {
            size,
            samples: 3,
            pixels: &rgb,
            name: &name,
        }
    };
    grana_tiff::write(path, &picture).map_err(|error| error.to_string())
}

fn strip_alpha(pixels: &[u16]) -> Vec<u16> {
    let mut rgb = Vec::with_capacity(pixels.len() / 4 * 3);
    for pixel in pixels.as_chunks::<4>().0 {
        rgb.extend_from_slice(&pixel[..3]);
    }
    rgb
}
