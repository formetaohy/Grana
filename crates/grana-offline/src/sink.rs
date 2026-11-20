use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use grana_archive::{Channel, Fill, ImageSpec};
use grana_render::Pixels;
use grana_scene::Scene;
use grana_video::{Recorder, Sequence};
use grana_volume::{Extent, Level, Spacing};

use crate::quantize::{Depth, Quantizer};

const CHANNELS: [(&str, [f32; 3]); 4] = [
    ("Red", [1.0, 0.0, 0.0]),
    ("Green", [0.0, 1.0, 0.0]),
    ("Blue", [0.0, 0.0, 1.0]),
    ("Alpha", [1.0, 1.0, 1.0]),
];

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
                let name = picture_name(scene, time);
                let samples = samples(transparent);
                let channels = CHANNELS[..samples]
                    .iter()
                    .map(|(name, tint)| Channel {
                        name,
                        tint: *tint,
                        range: grana_volume::Intensity::new(0.0, 1.0),
                    })
                    .collect::<Vec<_>>();
                let levels = [Level::new(
                    Extent::new(size[0], size[1], 1),
                    Spacing::new(physical, physical, physical),
                )];
                let fill = |_: usize,
                            _: u32,
                            channel: usize,
                            origin: [u32; 3],
                            extent: [u32; 3],
                            data: &mut [f32]|
                 -> Result<(), String> {
                    for line in 0..extent[1] {
                        let row = picture.row(origin[1] + line).as_chunks::<16>().0;
                        let start = (line * extent[0]) as usize;
                        let values = &mut data[start..start + extent[0] as usize];
                        for (value, texel) in values.iter_mut().zip(&row[origin[0] as usize..]) {
                            *value = f32::from_le_bytes(
                                texel[channel * 4..channel * 4 + 4]
                                    .try_into()
                                    .expect("a picture sample spans four bytes"),
                            ) * f32::from(u16::MAX);
                        }
                    }
                    Ok(())
                };
                let cancel = AtomicBool::new(false);
                let mut progress = |_: usize, _: usize| {};
                grana_tiff::write_archive(
                    path,
                    &mut ImageSpec {
                        name: &name,
                        channels: &channels,
                        levels: &levels,
                        placement: [0.0; 3],
                        frames: 1,
                        interval: Some(scene.dataset.frame_interval()),
                        sample: grana_volume::VoxelFormat::U16,
                        fill: Fill::EveryLevel(&fill),
                        cancel: &cancel,
                        progress: &mut progress,
                    },
                )
                .map(|_| ())
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
