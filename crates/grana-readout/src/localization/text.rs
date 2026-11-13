use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use grana_volume::Level;

use super::{FRAME_TOLERANCE, Localization, Table, frame, narrow, normalize};
use crate::error::Error;

const PROGRESS_STRIDE: usize = 8192;
const UNIT_NANOMETER: f64 = 0.001;
const UNIT_MICROMETER: f64 = 1.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Unit {
    Nanometer,
    Micrometer,
    Pixel,
}

#[derive(Clone, Copy)]
enum Space {
    X,
    Y,
    Z,
}

impl Space {
    const ALL: [Self; 3] = [Self::X, Self::Y, Self::Z];

    fn index(self) -> usize {
        match self {
            Self::X => 0,
            Self::Y => 1,
            Self::Z => 2,
        }
    }
}

struct Column {
    name: String,
    unit: Option<Unit>,
}

struct Columns {
    cells: Vec<Column>,
    delimiter: u8,
    x: usize,
    y: usize,
    z: Option<usize>,
    frame: Option<usize>,
    quality: Option<usize>,
    precision_x: Option<usize>,
    precision_y: Option<usize>,
    lateral: Option<usize>,
    vertical: Option<usize>,
    psf: Option<usize>,
    psf_x: Option<usize>,
    psf_y: Option<usize>,
    group: Option<usize>,
}

impl Columns {
    fn space(&self, space: Space) -> Option<usize> {
        match space {
            Space::X => Some(self.x),
            Space::Y => Some(self.y),
            Space::Z => self.z,
        }
    }

    fn sigma(&self, space: Space) -> Option<usize> {
        match space {
            Space::X => self
                .precision_x
                .or(self.lateral)
                .or(self.psf)
                .or(self.psf_x),
            Space::Y => self
                .precision_y
                .or(self.lateral)
                .or(self.psf)
                .or(self.psf_y),
            Space::Z => self.vertical,
        }
    }

    fn unit(&self, column: Option<usize>) -> Option<Unit> {
        column
            .and_then(|column| self.cells.get(column))
            .and_then(|cell| cell.unit)
    }
}

pub(super) fn has_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            ["csv", "tsv", "txt", "dat"]
                .iter()
                .any(|known| known.eq_ignore_ascii_case(extension))
        })
}

pub(super) fn read(
    path: &Path,
    level: Level,
    frames: u32,
    report: &mut dyn FnMut(u64, u64) -> bool,
) -> Result<Option<Table>, Error> {
    let total = std::fs::metadata(path)
        .map_err(|error| Error::table(path, error.to_string()))?
        .len();
    let columns = header(path, open(path)?)?;
    if columns.frame.is_none() && frames > 1 {
        return Err(Error::table(
            path,
            format!("the table carries no frame column for {frames} frames"),
        ));
    }
    let Some(peaks) = peaks(path, &mut open(path)?, &columns, &mut |done| {
        report(done, total.saturating_mul(2))
    })?
    else {
        return Ok(None);
    };
    let scales = scales(&columns, level, peaks);
    let Some(localizations) = rows(path, &mut open(path)?, &columns, &scales, &mut |done| {
        report(total.saturating_add(done), total.saturating_mul(2))
    })?
    else {
        return Ok(None);
    };
    let extent = level.extent();
    let pixel = (level.voxel()[0] * 1000.0).max(f32::MIN_POSITIVE);
    Ok(Some(Table::new(
        localizations,
        pixel,
        extent.x(),
        extent.y(),
        columns.z.is_some(),
        columns.quality.is_some(),
    )))
}

fn open(path: &Path) -> Result<BufReader<File>, Error> {
    let file = File::open(path).map_err(|error| Error::open(path, error.to_string()))?;
    Ok(BufReader::new(file))
}

fn header(path: &Path, mut reader: BufReader<File>) -> Result<Columns, Error> {
    let mut line = String::new();
    loop {
        line.clear();
        let read = reader
            .read_line(&mut line)
            .map_err(|error| Error::table(path, error.to_string()))?;
        if read == 0 {
            return Err(Error::table(path, "the table carries no header"));
        }
        let text = line.trim();
        if !text.is_empty() && !text.starts_with('#') {
            break;
        }
    }
    let text = line.trim().trim_start_matches('\u{feff}');
    let delimiter = delimiter(text);
    let fields = split(text, delimiter);
    if fields.len() < 2 {
        return Err(Error::table(
            path,
            "the header names fewer than two columns",
        ));
    }
    let mut cells = Vec::with_capacity(fields.len());
    for field in fields {
        let (name, unit) = split_unit(field);
        if name.is_empty() {
            return Err(Error::table(path, "the header carries an unnamed column"));
        }
        cells.push(Column { name, unit });
    }
    let find = |wanted: &[&str]| {
        cells
            .iter()
            .position(|column| wanted.contains(&column.name.as_str()))
    };
    let x = find(&["x", "posx", "positionx", "xpos", "xcoord", "xcoordinate"])
        .ok_or_else(|| Error::table(path, "the table carries no x column"))?;
    let y = find(&["y", "posy", "positiony", "ypos", "ycoord", "ycoordinate"])
        .ok_or_else(|| Error::table(path, "the table carries no y column"))?;
    if x == y {
        return Err(Error::table(path, "the table names x and y once"));
    }
    let z = find(&["z", "posz", "positionz", "zpos", "zcoord", "zcoordinate"]);
    let frame = find(&["frame", "t", "time", "frameindex", "frameid", "image"]);
    let quality = find(&["photons", "photon", "intensity", "amplitude", "brightness"]);
    let precision_x = find(&["lpx", "precisionx", "localizationprecisionx"]);
    let precision_y = find(&["lpy", "precisiony", "localizationprecisiony"]);
    let lateral = find(&["uncertaintyxy", "uncertaintylateral", "precisionlateral"]);
    let vertical = find(&[
        "lpz",
        "uncertaintyz",
        "uncertaintyaxial",
        "precisionaxial",
        "sz",
        "sigmaz",
        "precisionz",
        "localizationprecisionz",
    ]);
    let psf = find(&["sigma", "psfsigma"]);
    let psf_x = find(&["sx", "sigmax", "sigma1"]);
    let psf_y = find(&["sy", "sigmay", "sigma2"]);
    let group = find(&["group", "groupid"]);
    Ok(Columns {
        cells,
        delimiter,
        x,
        y,
        z,
        frame,
        quality,
        precision_x,
        precision_y,
        lateral,
        vertical,
        psf,
        psf_x,
        psf_y,
        group,
    })
}

fn delimiter(line: &str) -> u8 {
    let count = |character: char| line.matches(character).count();
    let candidates = [(b'\t', count('\t')), (b',', count(',')), (b';', count(';'))];
    candidates
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .filter(|(_, count)| *count > 0)
        .map_or(b' ', |(delimiter, _)| delimiter)
}

fn split(text: &str, delimiter: u8) -> Vec<&str> {
    if delimiter == b' ' {
        return text.split_whitespace().collect();
    }
    text.split(delimiter as char).map(str::trim).collect()
}

fn unquote(cell: &str) -> &str {
    cell.trim()
        .trim_start_matches(['"', '\''])
        .trim_end_matches(['"', '\''])
}

fn split_unit(cell: &str) -> (String, Option<Unit>) {
    let cell = unquote(cell).trim();
    for (open, close) in [('[', ']'), ('(', ')')] {
        if let Some(start) = cell.find(open)
            && let Some(end) = cell[start..].find(close)
        {
            let unit = parse_unit(&cell[start + 1..start + end]);
            return (normalize(cell[..start].trim()), unit);
        }
    }
    if let Some((head, tail)) = cell.rsplit_once(char::is_whitespace)
        && let Some(unit) = parse_unit(tail)
    {
        return (normalize(head.trim()), Some(unit));
    }
    (normalize(cell), None)
}

fn parse_unit(token: &str) -> Option<Unit> {
    match normalize(token).as_str() {
        "nm" | "nanometer" | "nanometers" | "nanometre" | "nanometres" => Some(Unit::Nanometer),
        "um" | "micrometer" | "micrometers" | "micrometre" | "micrometres" | "micron"
        | "microns" => Some(Unit::Micrometer),
        "px" | "pix" | "pixel" | "pixels" => Some(Unit::Pixel),
        _ => None,
    }
}

struct Scales {
    space: [f64; 3],
    sigma: [f64; 3],
    fallback: [f32; 3],
}

fn peaks(
    path: &Path,
    reader: &mut BufReader<File>,
    columns: &Columns,
    report: &mut dyn FnMut(u64) -> bool,
) -> Result<Option<[f64; 3]>, Error> {
    let mut peaks = [0.0f64; 3];
    let finished = scan(path, reader, columns.delimiter, report, |fields, line| {
        for space in Space::ALL {
            let Some(column) = columns.space(space) else {
                continue;
            };
            let value = number(path, fields, column, line)?;
            peaks[space.index()] = peaks[space.index()].max(value.abs());
        }
        Ok(true)
    })?;
    Ok(finished.then_some(peaks))
}

fn scales(columns: &Columns, level: Level, peaks: [f64; 3]) -> Scales {
    let extent = level.world_extent();
    let voxel = level.voxel();
    let fallback = [0, 1, 2].map(|axis| 0.5 * voxel[axis].max(f32::MIN_POSITIVE));
    let space_scale = [0, 1, 2].map(|axis| {
        let space = Space::ALL[axis];
        let step = f64::from(voxel[axis]);
        match columns.unit(columns.space(space)) {
            Some(unit) => step_of(unit, step),
            None => infer(step, f64::from(extent[axis]), peaks[axis]),
        }
    });
    let sigma = [0, 1, 2].map(|axis| {
        let space = Space::ALL[axis];
        let step = f64::from(voxel[axis]);
        match columns.unit(columns.sigma(space)) {
            Some(unit) => step_of(unit, step),
            None => space_scale[axis],
        }
    });
    Scales {
        space: space_scale,
        sigma,
        fallback,
    }
}

fn step_of(unit: Unit, step: f64) -> f64 {
    match unit {
        Unit::Nanometer => UNIT_NANOMETER,
        Unit::Micrometer => UNIT_MICROMETER,
        Unit::Pixel => step,
    }
}

fn infer(step: f64, extent: f64, peak: f64) -> f64 {
    if peak <= 0.0 || extent <= 0.0 {
        return UNIT_MICROMETER;
    }
    [UNIT_MICROMETER, UNIT_NANOMETER, step]
        .into_iter()
        .min_by(|left, right| {
            let distance = |factor: f64| (factor * peak / extent).ln().abs();
            distance(*left).total_cmp(&distance(*right))
        })
        .expect("a unit scale holds a candidate")
}

fn rows(
    path: &Path,
    reader: &mut BufReader<File>,
    columns: &Columns,
    scales: &Scales,
    report: &mut dyn FnMut(u64) -> bool,
) -> Result<Option<Vec<Localization>>, Error> {
    let mut localizations = Vec::new();
    let finished = scan(path, reader, columns.delimiter, report, |fields, line| {
        let mut center = [0.0f32; 3];
        for space in Space::ALL {
            let Some(column) = columns.space(space) else {
                continue;
            };
            let axis = space.index();
            let value = number(path, fields, column, line)?;
            center[axis] = narrow(path, value * scales.space[axis], line)?;
        }
        let mut sigma = scales.fallback;
        for space in Space::ALL {
            let axis = space.index();
            let Some(column) = columns.sigma(space) else {
                continue;
            };
            let value = number(path, fields, column, line)?;
            if value > 0.0 {
                sigma[axis] = narrow(path, value * scales.sigma[axis], line)?;
            }
        }
        let quality = match columns.quality {
            Some(column) => narrow(path, number(path, fields, column, line)?, line)?,
            None => 0.0,
        };
        let time = match columns.frame {
            Some(column) => frame(path, number(path, fields, column, line)?, line)?,
            None => 0,
        };
        let group = match columns.group {
            Some(column) => group(path, number(path, fields, column, line)?, line)?,
            None => None,
        };
        localizations.push(Localization {
            time,
            center,
            sigma,
            quality,
            group,
        });
        Ok(true)
    })?;
    Ok(finished.then_some(localizations))
}

fn group(path: &Path, value: f64, line: usize) -> Result<Option<u32>, Error> {
    if value < 0.0 {
        return Ok(None);
    }
    if (value - value.round()).abs() > FRAME_TOLERANCE {
        return Err(Error::table(
            path,
            format!("the row {line} carries the group {value}"),
        ));
    }
    Ok(u32::try_from(value.round() as u64).ok())
}

fn number(path: &Path, fields: &[&str], column: usize, line: usize) -> Result<f64, Error> {
    let field = fields.get(column).ok_or_else(|| {
        Error::table(
            path,
            format!("line {line} carries fewer values than the header names"),
        )
    })?;
    let value = unquote(field)
        .parse::<f64>()
        .map_err(|_| Error::table(path, format!("line {line} carries the value {field}")))?;
    if !value.is_finite() {
        return Err(Error::table(
            path,
            format!("line {line} carries the value {field}"),
        ));
    }
    Ok(value)
}

fn scan<F>(
    path: &Path,
    reader: &mut BufReader<File>,
    delimiter: u8,
    report: &mut dyn FnMut(u64) -> bool,
    mut visit: F,
) -> Result<bool, Error>
where
    F: FnMut(&[&str], usize) -> Result<bool, Error>,
{
    let mut line = String::new();
    let mut index = 0usize;
    let mut read = 0u64;
    let mut header = true;
    loop {
        line.clear();
        let bytes = reader
            .read_line(&mut line)
            .map_err(|error| Error::table(path, error.to_string()))?;
        if bytes == 0 {
            report(read);
            return Ok(true);
        }
        read += bytes as u64;
        index += 1;
        let text = line.trim();
        if text.is_empty() || text.starts_with('#') {
            continue;
        }
        if header {
            header = false;
            continue;
        }
        if !visit(&split(text, delimiter), index)? {
            return Ok(false);
        }
        if index.is_multiple_of(PROGRESS_STRIDE) && !report(read) {
            return Ok(false);
        }
    }
}
