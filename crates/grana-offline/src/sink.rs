use std::path::{Path, PathBuf};

use grana_render::Image16;
use grana_scene::Scene;
use grana_video::{Recorder, Sequence};

const BAYER: [[u8; 8]; 8] = [
    [0, 32, 8, 40, 2, 34, 10, 42],
    [48, 16, 56, 24, 50, 18, 58, 26],
    [12, 44, 4, 36, 14, 46, 6, 38],
    [60, 28, 52, 20, 62, 30, 54, 22],
    [3, 35, 11, 43, 1, 33, 9, 41],
    [51, 19, 59, 27, 49, 17, 57, 25],
    [15, 47, 7, 39, 13, 45, 5, 37],
    [63, 31, 55, 23, 61, 29, 53, 21],
];

pub(crate) enum Sink {
    Png { path: PathBuf },
    Tiff { path: PathBuf },
    Movie { path: PathBuf, recorder: Recorder },
    Sequence { sequence: Sequence },
}

impl Sink {
    pub(crate) fn push(
        &mut self,
        time: u32,
        size: [u32; 2],
        image: &Image16,
        transparent: bool,
    ) -> Result<(), String> {
        assert_eq!(image.size(), size, "the picture covers the export");
        let pixels = image.pixels();
        match self {
            Self::Png { path } => encode16(path, size, pixels, transparent),
            Self::Tiff { .. } => {
                unreachable!("a sixteen bit picture is written from the rendered frame")
            }
            Self::Movie { recorder, .. } => {
                let rgba = to_eight(size, pixels, transparent);
                recorder.push(&rgba).map_err(|error| error.to_string())
            }
            Self::Sequence { sequence } => {
                let png = png16(size, pixels, transparent)?;
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

fn encode16(path: &Path, size: [u32; 2], pixels: &[u16], transparent: bool) -> Result<(), String> {
    if transparent {
        grana_raster::write16(path, size, 4, pixels).map_err(String::from)
    } else {
        let rgb = strip_alpha(pixels);
        grana_raster::write16(path, size, 3, &rgb).map_err(String::from)
    }
}

fn png16(size: [u32; 2], pixels: &[u16], transparent: bool) -> Result<Vec<u8>, String> {
    if transparent {
        grana_raster::encode16(size, 4, pixels).map_err(String::from)
    } else {
        let rgb = strip_alpha(pixels);
        grana_raster::encode16(size, 3, &rgb).map_err(String::from)
    }
}

fn to_eight(size: [u32; 2], pixels: &[u16], transparent: bool) -> Vec<u8> {
    assert_eq!(
        pixels.len(),
        size[0] as usize * size[1] as usize * 4,
        "the picture covers every pixel of the export"
    );
    let mut rgba = Vec::with_capacity(pixels.len());
    for (index, pixel) in pixels.as_chunks::<4>().0.iter().enumerate() {
        let x = index % size[0] as usize;
        let y = index / size[0] as usize;
        let threshold = f32::from(BAYER[y % 8][x % 8]) / 64.0 - 0.5;
        for sample in &pixel[..3] {
            let value = f32::from(*sample) / 65535.0 * 255.0 + threshold;
            rgba.push(value.round().clamp(0.0, 255.0) as u8);
        }
        rgba.push(if transparent {
            (u32::from(pixel[3]) * 255 / 65535) as u8
        } else {
            255
        });
    }
    rgba
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
