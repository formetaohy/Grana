use std::path::Path;

use grana_volume::Level;
use hdf5_pure::{Datatype, File};

use super::{Localization, frame, narrow, normalize};
use crate::error::Error;
use crate::number;

const TABLE: [&str; 2] = ["locs", "locs/table"];
const METADATA: &str = "metadata";
const NANOMETER: f64 = 0.001;
const PROGRESS_STRIDE: usize = 65536;

pub(super) fn has_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            ["h5", "hdf5"]
                .iter()
                .any(|known| known.eq_ignore_ascii_case(extension))
        })
}

pub(super) fn read(
    path: &Path,
    level: Level,
    frames: u32,
    report: &mut dyn FnMut(u64, u64) -> bool,
) -> Result<Option<Vec<Localization>>, Error> {
    let file = File::open_streaming(path).map_err(|error| Error::table(path, error.to_string()))?;
    let table = Table::load(&file, path)?;
    let columns = Columns::new(&table, path, frames)?;
    let pixel = pixel_size(&file, path, level);
    let voxel = level.voxel();
    let fallback = [0, 1, 2].map(|axis| 0.5 * voxel[axis].max(f32::MIN_POSITIVE));
    let total = table.rows as u64;
    if !report(0, total) {
        return Ok(None);
    }
    let mut localizations = Vec::with_capacity(table.rows);
    for row in 0..table.rows {
        if row.is_multiple_of(PROGRESS_STRIDE) && !report(row as u64, total) {
            return Ok(None);
        }
        let center = [
            narrow(path, table.read(columns.x, row, path)? * pixel, row)?,
            narrow(path, table.read(columns.y, row, path)? * pixel, row)?,
            match columns.z {
                Some(column) => narrow(path, table.read(column, row, path)? * NANOMETER, row)?,
                None => 0.0,
            },
        ];
        let mut lateral = [0.0f32; 2];
        for axis in 0..2 {
            lateral[axis] = match first(
                &table,
                [columns.lateral[axis], columns.psf[axis]],
                row,
                path,
            )? {
                Some(value) => narrow(path, value * pixel, row)?,
                None => fallback[axis],
            };
        }
        let sigma = [
            lateral[0],
            lateral[1],
            match first(
                &table,
                [columns.axial_precision, columns.axial_psf],
                row,
                path,
            )? {
                Some(value) => narrow(path, value * NANOMETER, row)?,
                None if columns.z.is_some() => fallback[2],
                None => 0.5 * (lateral[0] + lateral[1]),
            },
        ];
        let quality = match columns.photons {
            Some(column) => narrow(path, table.read(column, row, path)?, row)?,
            None => 0.0,
        };
        let time = match columns.frame {
            Some(column) => frame(path, table.read(column, row, path)?, row)?,
            None => 0,
        };
        let group = match columns.group {
            Some(column) => group(path, table.read(column, row, path)?, row)?,
            None => None,
        };
        localizations.push(Localization {
            time,
            center,
            sigma,
            quality,
            group,
        });
    }
    report(total, total);
    Ok(Some(localizations))
}

fn first(
    table: &Table,
    columns: [Option<usize>; 2],
    row: usize,
    path: &Path,
) -> Result<Option<f64>, Error> {
    for column in columns.into_iter().flatten() {
        let value = table.read(column, row, path)?;
        if value > 0.0 {
            return Ok(Some(value));
        }
    }
    Ok(None)
}

fn group(path: &Path, value: f64, row: usize) -> Result<Option<u32>, Error> {
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
    fn new(table: &Table, path: &Path, frames: u32) -> Result<Self, Error> {
        let x = table
            .find(&["x"])
            .ok_or_else(|| Error::table(path, "the table carries no x column"))?;
        let y = table
            .find(&["y"])
            .ok_or_else(|| Error::table(path, "the table carries no y column"))?;
        let frame = table.find(&["frame"]);
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
            z: table.find(&["z"]),
            lateral: [table.find(&["lpx"]), table.find(&["lpy"])],
            psf: [table.find(&["sx"]), table.find(&["sy"])],
            axial_precision: table.find(&["lpz"]),
            axial_psf: table.find(&["sz"]),
            photons: table.find(&["photons", "photon"]),
            group: table.find(&["group"]),
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

struct Table {
    rows: usize,
    record: usize,
    members: Vec<Member>,
    raw: Vec<u8>,
}

impl Table {
    fn load(file: &File, path: &Path) -> Result<Self, Error> {
        let dataset = TABLE
            .iter()
            .find_map(|name| file.dataset(name).ok())
            .ok_or_else(|| Error::table(path, "the file holds no localization table"))?;
        let shape = dataset
            .shape()
            .map_err(|error| Error::table(path, error.to_string()))?;
        let rows = shape.first().copied().unwrap_or(0) as usize;
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
        let raw = dataset
            .read_raw()
            .map_err(|error| Error::table(path, error.to_string()))?;
        if raw.len() < rows * record {
            return Err(Error::table(
                path,
                "the localization table holds fewer rows than it declares",
            ));
        }
        Ok(Self {
            rows,
            record,
            members,
            raw,
        })
    }

    fn find(&self, names: &[&str]) -> Option<usize> {
        self.members
            .iter()
            .position(|member| names.contains(&member.name.as_str()))
    }

    fn read(&self, column: usize, row: usize, path: &Path) -> Result<f64, Error> {
        let Some(member) = self.members.get(column) else {
            return Err(Error::table(path, format!("the row {row} reads no column")));
        };
        let width = number::size(&member.datatype).expect("a member holds a numeric width");
        let start = row * self.record + member.offset;
        self.raw
            .get(start..start + width)
            .and_then(|bytes| number::decode(&member.datatype, bytes))
            .ok_or_else(|| {
                Error::table(
                    path,
                    format!("the row {row} carries no readable {}", member.name),
                )
            })
    }
}
