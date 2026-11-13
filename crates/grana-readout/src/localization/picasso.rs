use std::path::{Path, PathBuf};

use grana_volume::Level;
use hdf5_pure::{Dataset, Datatype, File};

use super::{BLOCK, Header, Localization, Place, Row, frame, narrow, normalize};
use crate::error::Error;
use crate::number;

const TABLE: [&str; 2] = ["locs", "locs/table"];
const METADATA: &str = "metadata";
const NANOMETER: f64 = 0.001;

pub(super) fn has_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            ["h5", "hdf5"]
                .iter()
                .any(|known| known.eq_ignore_ascii_case(extension))
        })
}

pub(super) fn open(path: &Path, level: Level, frames: u32) -> Result<(Blocks, Header), Error> {
    let file = File::open_streaming(path).map_err(|error| Error::table(path, error.to_string()))?;
    let dataset = TABLE
        .iter()
        .find_map(|name| file.dataset(name).ok())
        .ok_or_else(|| Error::table(path, "the file holds no localization table"))?;
    let shape = dataset
        .shape()
        .map_err(|error| Error::table(path, error.to_string()))?;
    let rows = shape.first().copied().unwrap_or(0);
    if rows == 0 {
        return Err(Error::table(path, "the table holds no localization"));
    }
    let datatype = dataset
        .datatype()
        .map_err(|error| Error::table(path, error.to_string()))?;
    let Datatype::Compound { size, members } = datatype else {
        return Err(Error::table(
            path,
            "the localization table carries no named columns",
        ));
    };
    let record = size as usize;
    if record == 0 {
        return Err(Error::table(
            path,
            "the localization table carries empty records",
        ));
    }
    let members = members
        .into_iter()
        .filter(|member| number::size(&member.datatype).is_some())
        .map(|member| Member {
            name: normalize(&member.name),
            offset: member.byte_offset as usize,
            datatype: member.datatype,
        })
        .collect::<Vec<_>>();
    let decoder = Decoder { members, record };
    let columns = Columns::new(&decoder, path, frames)?;
    let pixel = pixel_size(&file, path, level);
    let [width, height] = field(&file, path, level);
    let [first, last] = match columns.frame {
        Some(column) => [
            probe(&dataset, &decoder, column, 0, path)?,
            probe(&dataset, &decoder, column, rows - 1, path)?,
        ],
        None => [0, 0],
    };
    let voxel = level.voxel();
    let header = Header {
        rows,
        first,
        last,
        pixel: (pixel * 1000.0) as f32,
        width,
        height,
        volume: columns.z.is_some(),
        photons: columns.photons.is_some(),
    };
    let blocks = Blocks {
        dataset,
        decoder,
        columns,
        pixel,
        fallback: [0, 1, 2].map(|axis| 0.5 * voxel[axis].max(f32::MIN_POSITIVE)),
        next: 0,
        total: rows,
        path: path.to_path_buf(),
    };
    Ok((blocks, header))
}

fn probe(
    dataset: &Dataset,
    decoder: &Decoder,
    column: usize,
    at: u64,
    path: &Path,
) -> Result<u32, Error> {
    let raw = dataset
        .read_raw_rows(at, 1)
        .map_err(|error| Error::table(path, error.to_string()))?;
    if raw.len() < decoder.record {
        return Err(Error::table(
            path,
            format!("the row {at} is missing from the table"),
        ));
    }
    let value = decoder.value(&raw, 0, column).ok_or_else(|| {
        Error::table(
            path,
            format!("the row {at} carries no readable {}", decoder.name(column)),
        )
    })?;
    frame(path, value, at)
}

pub(super) struct Blocks {
    dataset: Dataset,
    decoder: Decoder,
    columns: Columns,
    pixel: f64,
    fallback: [f32; 3],
    next: u64,
    total: u64,
    path: PathBuf,
}

impl Blocks {
    pub(super) fn read(
        &mut self,
        place: &Place,
        out: &mut Vec<Localization>,
    ) -> Result<usize, Error> {
        if self.next >= self.total {
            return Ok(0);
        }
        let count = BLOCK.min((self.total - self.next) as usize);
        let raw = self
            .dataset
            .read_raw_rows(self.next, count as u64)
            .map_err(|error| Error::table(&self.path, error.to_string()))?;
        if raw.len() < count * self.decoder.record {
            return Err(Error::table(
                &self.path,
                "the localization table holds fewer rows than it declares",
            ));
        }
        for index in 0..count {
            let at = self.next + index as u64;
            let value = |column: usize| {
                self.decoder.value(&raw, index, column).ok_or_else(|| {
                    Error::table(
                        &self.path,
                        format!(
                            "the row {at} carries no readable {}",
                            self.decoder.name(column)
                        ),
                    )
                })
            };
            let center = [
                narrow(&self.path, value(self.columns.x)? * self.pixel, at)?,
                narrow(&self.path, value(self.columns.y)? * self.pixel, at)?,
                match self.columns.z {
                    Some(column) => narrow(&self.path, value(column)? * NANOMETER, at)?,
                    None => 0.0,
                },
            ];
            let mut sigma = [0.0f32; 3];
            for (axis, value) in sigma.iter_mut().take(2).enumerate() {
                let columns = [self.columns.lateral[axis], self.columns.psf[axis]];
                *value = match positive(&self.decoder, columns, &raw, index) {
                    Some(found) => narrow(&self.path, found * self.pixel, at)?,
                    None => self.fallback[axis],
                };
            }
            let columns = [self.columns.axial_precision, self.columns.axial_psf];
            sigma[2] = match positive(&self.decoder, columns, &raw, index) {
                Some(value) => narrow(&self.path, value * NANOMETER, at)?,
                None if self.columns.z.is_some() => self.fallback[2],
                None => 0.5 * (sigma[0] + sigma[1]),
            };
            let quality = match self.columns.photons {
                Some(column) => narrow(&self.path, value(column)?, at)?,
                None => 0.0,
            };
            let time = match self.columns.frame {
                Some(column) => frame(&self.path, value(column)?, at)?,
                None => 0,
            };
            let group = match self.columns.group {
                Some(column) => group(&self.path, value(column)?, at)?,
                None => None,
            };
            out.push(place.row(
                &self.path,
                at,
                Row {
                    time,
                    center,
                    sigma,
                    quality,
                    group,
                },
            )?);
        }
        self.next += count as u64;
        Ok(count)
    }
}

fn positive(decoder: &Decoder, columns: [Option<usize>; 2], raw: &[u8], row: usize) -> Option<f64> {
    columns
        .into_iter()
        .flatten()
        .filter_map(|column| decoder.value(raw, row, column))
        .find(|value| *value > 0.0)
}

fn group(path: &Path, value: f64, row: u64) -> Result<Option<u32>, Error> {
    if !value.is_finite() || value < 0.0 {
        return Ok(None);
    }
    if (value - value.round()).abs() > super::FRAME_TOLERANCE {
        return Err(Error::table(
            path,
            format!("the row {row} carries the group {value}"),
        ));
    }
    Ok(u32::try_from(value.round() as u64).ok())
}

struct Columns {
    x: usize,
    y: usize,
    frame: Option<usize>,
    z: Option<usize>,
    lateral: [Option<usize>; 2],
    psf: [Option<usize>; 2],
    axial_precision: Option<usize>,
    axial_psf: Option<usize>,
    photons: Option<usize>,
    group: Option<usize>,
}

impl Columns {
    fn new(decoder: &Decoder, path: &Path, frames: u32) -> Result<Self, Error> {
        let x = decoder
            .find(&["x"])
            .ok_or_else(|| Error::table(path, "the table carries no x column"))?;
        let y = decoder
            .find(&["y"])
            .ok_or_else(|| Error::table(path, "the table carries no y column"))?;
        let frame = decoder.find(&["frame"]);
        if frame.is_none() && frames > 1 {
            return Err(Error::table(
                path,
                format!("the table carries no frame column for {frames} frames"),
            ));
        }
        Ok(Self {
            x,
            y,
            frame,
            z: decoder.find(&["z"]),
            lateral: [decoder.find(&["lpx"]), decoder.find(&["lpy"])],
            psf: [decoder.find(&["sx"]), decoder.find(&["sy"])],
            axial_precision: decoder.find(&["lpz"]),
            axial_psf: decoder.find(&["sz"]),
            photons: decoder.find(&["photons", "photon"]),
            group: decoder.find(&["group"]),
        })
    }
}

fn pixel_size(file: &File, path: &Path, level: Level) -> f64 {
    metadata(file, path)
        .and_then(|text| scalar(&text, "Pixelsize"))
        .map_or(f64::from(level.voxel()[0]), |nanometers| {
            nanometers * NANOMETER
        })
}

fn field(file: &File, path: &Path, level: Level) -> [u32; 2] {
    let extent = level.extent();
    let metadata = metadata(file, path);
    let axis = |key: &str, fallback: u32| {
        metadata
            .as_deref()
            .and_then(|text| scalar(text, key))
            .and_then(|value| u32::try_from(value.round() as u64).ok())
            .filter(|value| *value > 0)
            .unwrap_or(fallback)
    };
    [axis("Width", extent.x()), axis("Height", extent.y())]
}

fn metadata(file: &File, path: &Path) -> Option<String> {
    let stored = file
        .dataset(METADATA)
        .ok()
        .and_then(|dataset| dataset.read_string().ok())
        .and_then(|strings| strings.into_iter().next());
    stored.or_else(|| std::fs::read_to_string(path.with_extension("yaml")).ok())
}

fn scalar(text: &str, key: &str) -> Option<f64> {
    let mut from = 0usize;
    while let Some(offset) = text[from..].find(key) {
        let start = from + offset + key.len();
        from = start;
        let tail = text[start..].trim_start();
        let tail = tail.trim_start_matches(['"', '\'']).trim_start();
        let Some(tail) = tail.strip_prefix(':').or_else(|| tail.strip_prefix('=')) else {
            continue;
        };
        let number = tail.trim_start().trim_start_matches(['"', '\'']);
        let end = number
            .find(|character: char| {
                !(character.is_ascii_digit() || matches!(character, '.' | '-' | '+' | 'e' | 'E'))
            })
            .unwrap_or(number.len());
        let value = number[..end].parse::<f64>().ok();
        if let Some(value) = value.filter(|value| value.is_finite() && *value > 0.0) {
            return Some(value);
        }
    }
    None
}

struct Member {
    name: String,
    offset: usize,
    datatype: Datatype,
}

struct Decoder {
    members: Vec<Member>,
    record: usize,
}

impl Decoder {
    fn find(&self, names: &[&str]) -> Option<usize> {
        self.members
            .iter()
            .position(|member| names.contains(&member.name.as_str()))
    }

    fn name(&self, column: usize) -> &str {
        &self.members[column].name
    }

    fn value(&self, raw: &[u8], row: usize, column: usize) -> Option<f64> {
        let member = &self.members[column];
        let width = number::size(&member.datatype)?;
        let start = row * self.record + member.offset;
        raw.get(start..start + width)
            .and_then(|bytes| number::decode(&member.datatype, bytes))
    }
}
