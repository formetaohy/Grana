use std::path::PathBuf;

use grana_render::Pixels;
use grana_scene::Scene;
use grana_video::{Recorder, Sequence};

use crate::quantize::{Depth, Quantizer};

pub(crate) enum Sink {
    Png { path: PathBuf },
    Tiff { path: PathBuf },
    Figure { path: PathBuf },
    Movie { path: PathBuf, recorder: Recorder },
    Sequence { sequence: Sequence },
}

impl Sink {
    pub(crate) fn push(
        &mut self,
        time: u32,
        picture: Pixels<'_>,
        scene: &Scene,
        physical: f32,
        transparent: bool,
    ) -> Result<(), String> {
        let size = picture.size();
        match self {
            Self::Png { path } => {
                let mut quantizer = Quantizer::new(
                    size,
                    samples(transparent),
                    Depth::Sixteen,
                    transparent,
                    false,
                );
                grana_raster::write16_stream(path, size, quantizer.samples(), &mut |line, row| {
                    quantizer.row16(line, picture.row(line), row);
                    Ok(())
                })
                .map_err(String::from)
            }
            Self::Tiff { path } => {
                let mut quantizer = Quantizer::new(
                    size,
                    samples(transparent),
                    Depth::Sixteen,
                    transparent,
                    false,
                );
                let name = picture_name(scene, time);
                let stream = grana_tiff::Stream16 {
                    size,
                    samples: u8::try_from(quantizer.samples())
                        .expect("a picture carries fewer than four samples"),
                    name: &name,
                    physical: Some([physical, physical]),
                    interval: Some(scene.dataset.frame_interval()),
                };
                grana_tiff::write16(path, &stream, &mut |line, row| {
                    quantizer.row16(line, picture.row(line), row);
                    Ok(())
                })
                .map_err(|error| error.to_string())
            }
            Self::Figure { .. } => {
                unreachable!("a vector figure is written from the resolved frame")
            }
            Self::Movie { recorder, .. } => {
                let mut quantizer = Quantizer::new(size, 4, Depth::Eight, transparent, true);
                let stride = quantizer.stride();
                let mut frame = vec![0u8; stride * size[1] as usize];
                for (line, row) in frame.chunks_mut(stride).enumerate() {
                    quantizer.row(line as u32, picture.row(line as u32), row);
                }
                recorder.push(frame).map_err(|error| error.to_string())
            }
            Self::Sequence { sequence } => {
                let mut quantizer = Quantizer::new(
                    size,
                    samples(transparent),
                    Depth::Sixteen,
                    transparent,
                    false,
                );
                let png =
                    grana_raster::encode16_stream(size, quantizer.samples(), &mut |line, row| {
                        quantizer.row16(line, picture.row(line), row);
                        Ok(())
                    })
                    .map_err(String::from)?;
                sequence.push(time, &png).map_err(|error| error.to_string())
            }
        }
    }

    pub(crate) fn finish(self) -> Result<PathBuf, String> {
        match self {
            Self::Png { path } => Ok(path),
            Self::Tiff { path } => Ok(path),
            Self::Figure { path } => Ok(path),
            Self::Movie { path, recorder } => {
                recorder.finish().map_err(|error| error.to_string())?;
                Ok(path)
            }
            Self::Sequence { sequence } => sequence.finish().map_err(|error| error.to_string()),
        }
    }

    pub(crate) fn discard(self) {
        match self {
            Self::Png { .. } | Self::Tiff { .. } | Self::Figure { .. } => {}
            Self::Movie { recorder, .. } => recorder.abort(),
            Self::Sequence { sequence } => sequence.discard(),
        }
    }
}

fn samples(transparent: bool) -> usize {
    if transparent { 4 } else { 3 }
}

fn picture_name(scene: &Scene, time: u32) -> String {
    if scene.dataset.frame_count() > 1 {
        format!("{} · t={time}", scene.dataset.name())
    } else {
        scene.dataset.name().to_string()
    }
}
