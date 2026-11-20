use grana_volume::{VoxelData, VoxelFormat};

use crate::sample::SampleType;

const PHOTOMETRIC_WHITE_IS_ZERO: u16 = 0;
const PHOTOMETRIC_BLACK_IS_ZERO: u16 = 1;
const PHOTOMETRIC_RGB: u16 = 2;
const PHOTOMETRIC_PALETTE: u16 = 3;
const PHOTOMETRIC_MASK: u16 = 4;
const PHOTOMETRIC_CMYK: u16 = 5;
const PHOTOMETRIC_YCBCR: u16 = 6;
const PHOTOMETRIC_CIELAB: u16 = 8;
const PHOTOMETRIC_ICCLAB: u16 = 9;
const PHOTOMETRIC_ITULAB: u16 = 10;

const DEFAULT_COEFFICIENTS: [f64; 3] = [0.299, 0.587, 0.114];

pub(crate) enum ColorModel {
    Gray { inverted: bool },
    Rgb,
    Palette { color_map: ColorMap },
    Mask,
    Cmyk,
    YCbCr { coefficients: [f64; 3] },
    Lab { space: LabSpace },
}

#[derive(Clone, Copy)]
pub(crate) enum LabSpace {
    Cie,
    Icc,
    Itu,
}

impl ColorModel {
    pub(crate) fn of(
        photometric: Option<u64>,
        samples: u32,
        sample: SampleType,
        values: &ColorValues,
        coded: bool,
    ) -> Result<Self, String> {
        let photometric = photometric.unwrap_or(u64::from(PHOTOMETRIC_WHITE_IS_ZERO));
        let photometric = u16::try_from(photometric)
            .map_err(|_| "the TIFF photometric interpretation is out of range".to_string())?;
        match photometric {
            PHOTOMETRIC_WHITE_IS_ZERO => {
                samples_with(samples, 1, &values.extra)?;
                Ok(Self::Gray { inverted: true })
            }
            PHOTOMETRIC_BLACK_IS_ZERO => {
                samples_with(samples, 1, &values.extra)?;
                Ok(Self::Gray { inverted: false })
            }
            PHOTOMETRIC_RGB => {
                samples_with(samples, 3, &values.extra)?;
                Ok(Self::Rgb)
            }
            PHOTOMETRIC_PALETTE => {
                samples_with(samples, 1, &values.extra)?;
                let Some(map) = values.color_map.as_deref() else {
                    return Err("the palette TIFF page carries no color map".to_string());
                };
                Ok(Self::Palette {
                    color_map: ColorMap::of(map, sample.bits)?,
                })
            }
            PHOTOMETRIC_MASK => {
                samples_with(samples, 1, &values.extra)?;
                Ok(Self::Mask)
            }
            PHOTOMETRIC_CMYK => {
                samples_with(samples, 4, &values.extra)?;
                Ok(Self::Cmyk)
            }
            PHOTOMETRIC_YCBCR => {
                let subsampling = values.subsampling.unwrap_or([2, 2]);
                if !coded && subsampling != [1, 1] {
                    return Err(format!(
                        "the TIFF YCbCr subsampling {}x{} is not supported",
                        subsampling[0], subsampling[1]
                    ));
                }
                samples_with(samples, 3, &values.extra)?;
                let coefficients = values
                    .coefficients
                    .as_deref()
                    .filter(|values| values.len() == 3)
                    .map(|values| [values[0], values[1], values[2]])
                    .unwrap_or(DEFAULT_COEFFICIENTS);
                Ok(Self::YCbCr { coefficients })
            }
            PHOTOMETRIC_CIELAB => {
                samples_with(samples, 3, &values.extra)?;
                Ok(Self::Lab {
                    space: LabSpace::Cie,
                })
            }
            PHOTOMETRIC_ICCLAB => {
                samples_with(samples, 3, &values.extra)?;
                Ok(Self::Lab {
                    space: LabSpace::Icc,
                })
            }
            PHOTOMETRIC_ITULAB => {
                samples_with(samples, 3, &values.extra)?;
                Ok(Self::Lab {
                    space: LabSpace::Itu,
                })
            }
            other => Err(format!(
                "the TIFF photometric interpretation {other} is not supported"
            )),
        }
    }

    pub(crate) fn channels(&self) -> u32 {
        match self {
            Self::Gray { .. } | Self::Mask => 1,
            Self::Rgb
            | Self::Palette { .. }
            | Self::Cmyk
            | Self::YCbCr { .. }
            | Self::Lab { .. } => 3,
        }
    }

    pub(crate) fn inverted(&self) -> bool {
        matches!(self, Self::Gray { inverted: true })
    }

    pub(crate) fn converts(&self) -> bool {
        matches!(
            self,
            Self::Palette { .. } | Self::Cmyk | Self::YCbCr { .. } | Self::Lab { .. }
        )
    }

    pub(crate) fn convert(
        &self,
        format: VoxelFormat,
        maximum: f32,
        samples: usize,
        data: &VoxelData,
    ) -> VoxelData {
        if let Self::Palette { color_map } = self
            && let VoxelData::U8(indices) = data
            && format == VoxelFormat::U8
        {
            return color_map.expand(indices);
        }
        let pixels = data.voxel_count() / samples;
        let count = pixels * 3;
        let scale = f64::from(maximum);
        let mut index = 0;
        VoxelData::from_values(format, count, || {
            let pixel = index / 3;
            let channel = index % 3;
            index += 1;
            let base = pixel * samples;
            let color = match self {
                Self::Palette { color_map } => {
                    let index = data.value(base) as usize;
                    f64::from(color_map.at(index)[channel])
                }
                Self::Cmyk => {
                    let [cyan, magenta, yellow, black] =
                        [0, 1, 2, 3].map(|slot| f64::from(data.value(base + slot)) / scale);
                    let keep = 1.0 - black;
                    [1.0 - cyan * keep, 1.0 - magenta * keep, 1.0 - yellow * keep][channel]
                }
                Self::YCbCr { coefficients } => {
                    let [kr, kg, kb] = *coefficients;
                    let y = f64::from(data.value(base)) / scale;
                    let cb = (f64::from(data.value(base + 1)) - f64::from(maximum) * 0.5) / scale;
                    let cr = (f64::from(data.value(base + 2)) - f64::from(maximum) * 0.5) / scale;
                    [
                        y + 2.0 * (1.0 - kr) * cr,
                        y - 2.0 * (1.0 - kb) * kb / kg * cb - 2.0 * (1.0 - kr) * kr / kg * cr,
                        y + 2.0 * (1.0 - kb) * cb,
                    ][channel]
                }
                Self::Lab { space } => {
                    let [lightness, a, b] =
                        [0, 1, 2].map(|slot| f64::from(data.value(base + slot)));
                    let color = lab_to_srgb(*space, [lightness, a, b], scale, maximum);
                    f64::from(color[channel])
                }
                _ => f64::from(data.value(base + channel)) / scale,
            };
            color.clamp(0.0, 1.0) * scale
        })
    }
}

pub(crate) struct ColorMap {
    entries: Vec<[f32; 3]>,
}

impl ColorMap {
    fn of(values: &[u64], bits: u16) -> Result<Self, String> {
        if bits == 0 || bits > 8 {
            return Err(format!(
                "the TIFF palette carries {bits} bits per sample, which exceeds the eight bits a color map holds"
            ));
        }
        let count = 1usize << bits;
        if values.len() != count * 3 {
            return Err(format!(
                "the TIFF color map holds {} entries where {count} colors need {}",
                values.len(),
                count * 3
            ));
        }
        let entries = (0..count)
            .map(|index| {
                [
                    values[index].min(u64::from(u16::MAX)) as u16,
                    values[count + index].min(u64::from(u16::MAX)) as u16,
                    values[2 * count + index].min(u64::from(u16::MAX)) as u16,
                ]
                .map(|component| f32::from(component) / 65535.0)
            })
            .collect();
        Ok(Self { entries })
    }

    fn at(&self, index: usize) -> [f32; 3] {
        self.entries[index.min(self.entries.len() - 1)]
    }

    fn expand(&self, indices: &[u8]) -> VoxelData {
        let scale = f64::from(u8::MAX);
        let table = std::array::from_fn::<[u8; 3], 256, _>(|index| {
            self.at(index)
                .map(|component| (f64::from(component).clamp(0.0, 1.0) * scale).round() as u8)
        });
        let mut values = vec![0u8; indices.len() * 3];
        for (index, color) in indices.iter().zip(values.as_chunks_mut::<3>().0) {
            *color = table[*index as usize];
        }
        VoxelData::U8(values)
    }
}

pub(crate) struct ColorValues {
    pub color_map: Option<Vec<u64>>,
    pub subsampling: Option<[u32; 2]>,
    pub coefficients: Option<Vec<f64>>,
    pub extra: Vec<u64>,
}

fn lab_to_srgb(space: LabSpace, lab: [f64; 3], scale: f64, maximum: f32) -> [f32; 3] {
    let _ = space;
    let maximum = f64::from(maximum);
    let lightness = lab[0] / maximum * 100.0;
    let a = lab[1] / maximum * 255.0 - 128.0;
    let b = lab[2] / maximum * 255.0 - 128.0;
    let fy = (lightness + 16.0) / 116.0;
    let fx = fy + a / 500.0;
    let fz = fy - b / 200.0;
    let x = 0.9642 * inverse(fx);
    let y = inverse(fy);
    let z = 0.8249 * inverse(fz);
    let _ = scale;
    [
        3.133_856_1 * x - 1.616_866_7 * y - 0.490_614_6 * z,
        -0.978_768_4 * x + 1.916_141_5 * y + 0.033_454 * z,
        0.071_945_3 * x - 0.228_991_4 * y + 1.405_242_7 * z,
    ]
    .map(|channel| {
        let linear = channel.clamp(0.0, 1.0);
        let srgb = if linear <= 0.003_130_8 {
            12.92 * linear
        } else {
            1.055 * linear.powf(1.0 / 2.4) - 0.055
        };
        srgb.clamp(0.0, 1.0) as f32
    })
}

fn inverse(value: f64) -> f64 {
    const EPSILON: f64 = 216.0 / 24389.0;
    const KAPPA: f64 = 24389.0 / 27.0;
    let cube = value * value * value;
    if cube > EPSILON {
        cube
    } else {
        (116.0 * value - 16.0) / KAPPA
    }
}

fn samples_with(samples: u32, base: u32, extra: &[u64]) -> Result<(), String> {
    if samples < base {
        return Err(format!(
            "the TIFF page holds {samples} samples per pixel where its color space needs {base}"
        ));
    }
    let extras = samples - base;
    if extras != extra.len() as u32 && !(extra.is_empty() && extras == 1) {
        return Err(format!(
            "the TIFF page holds {extras} extra samples but declares {}",
            extra.len()
        ));
    }
    if extra.iter().any(|value| *value > 2) {
        return Err("the TIFF extra samples carry an unknown meaning".to_string());
    }
    Ok(())
}
