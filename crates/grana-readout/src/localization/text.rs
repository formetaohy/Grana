use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use grana_volume::Level;

use super::{BLOCK, FRAME_TOLERANCE, Header, Localization, Place, Row, frame, narrow, normalize};
use crate::error::Error;

const UNIT_NANOMETER: f64 = 0.001;
const UNIT_MICROMETER: f64 = 1.0;
const PROGRESS_STRIDE: u64 = 4096;

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

pub(super) fn open(
    path: &Path,
    level: Level,
    frames: u32,
    report: &mut dyn FnMut(u64, u64) -> bool,
) -> Result<Option<(Blocks, Header)>, Error> {
    let mut lines = Lines::open(path)?;
    let columns = lines.header()?;
    if columns.frame.is_none() && frames > 1 {
        return Err(Error::table(
            path,
            format!("the table carries no frame column for {frames} frames"),
        ));
    }
    let Some(survey) = survey(path, &mut lines, &columns, report)? else {
        return Ok(None);
    };
    let scales = scales(&columns, level, survey.peaks);
    let mut lines = Lines::open(path)?;
    let columns = lines.header()?;
    let extent = level.extent();
    let header = Header {
        rows: survey.rows,
        first: survey.first,
        last: survey.last,
        pixel: (level.voxel()[0] * 1000.0).max(f32::MIN_POSITIVE),
        width: extent.x(),
        height: extent.y(),
        volume: columns.z.is_some(),
        photons: columns.quality.is_some(),
    };
    let blocks = Blocks {
        lines,
        columns,
        scales,
        path: path.to_path_buf(),
        read: 0,
    };
    Ok(Some((blocks, header)))
}

struct Lines {
    path: PathBuf,
    reader: BufReader<File>,
    text: String,
    number: usize,
}

impl Lines {
    fn open(path: &Path) -> Result<Self, Error> {
        let file = File::open(path).map_err(|error| Error::open(path, error.to_string()))?;
        Ok(Self {
            path: path.to_path_buf(),
            reader: BufReader::new(file),
            text: String::new(),
            number: 0,
        })
    }

    fn header(&mut self) -> Result<Columns, Error> {
        if self.next()?.is_none() {
            return Err(Error::table(self.path(), "the table carries no header"));
        }
        parse(self.path(), self.text())
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn text(&self) -> &str {
        self.text.trim()
    }

    fn next(&mut self) -> Result<Option<usize>, Error> {
        loop {
            self.text.clear();
            let bytes = self
                .reader
                .read_line(&mut self.text)
                .map_err(|error| Error::table(&self.path, error.to_string()))?;
            if bytes == 0 {
                return Ok(None);
            }
            self.number += 1;
            let text = self.text.trim();
            if text.is_empty() || text.starts_with('#') {
                continue;
            }
            return Ok(Some(self.number));
        }
    }
}

pub(super) struct Blocks {
    lines: Lines,
    columns: Columns,
    scales: Scales,
    path: PathBuf,
    read: u64,
}

impl Blocks {
    pub(super) fn read(
        &mut self,
        place: &Place,
        out: &mut Vec<Localization>,
    ) -> Result<usize, Error> {
        let mut added = 0usize;
        while added < BLOCK {
            let Some(line) = self.lines.next()? else {
                break;
            };
            let fields = split(self.lines.text(), self.columns.delimiter);
            let at = self.read + added as u64;
            let row = row(&self.path, &self.columns, &self.scales, &fields, line, at)?;
            out.push(place.row(&self.path, at, row)?);
            added += 1;
        }
        self.read += added as u64;
        Ok(added)
    }
}

struct Survey {
    peaks: [f64; 3],
    first: u32,
    last: u32,
    rows: u64,
}

fn survey(
    path: &Path,
    lines: &mut Lines,
    columns: &Columns,
    report: &mut dyn FnMut(u64, u64) -> bool,
) -> Result<Option<Survey>, Error> {
    let mut survey = Survey {
        peaks: [0.0; 3],
        first: 0,
        last: 0,
        rows: 0,
    };
    while let Some(line) = lines.next()? {
        let fields = split(lines.text(), columns.delimiter);
        for space in Space::ALL {
            let Some(column) = columns.space(space) else {
                continue;
            };
            let value = number(path, &fields, column, line)?;
            let axis = space.index();
            survey.peaks[axis] = survey.peaks[axis].max(value.abs());
        }
        if let Some(column) = columns.frame {
            let value = frame(path, number(path, &fields, column, line)?, survey.rows)?;
            if survey.rows == 0 {
                survey.first = value;
                survey.last = value;
            } else {
                survey.first = survey.first.min(value);
                survey.last = survey.last.max(value);
            }
        }
        survey.rows += 1;
        if survey.rows.is_multiple_of(PROGRESS_STRIDE) && !report(survey.rows, 0) {
            return Ok(None);
        }
    }
    if survey.rows == 0 {
        return Err(Error::table(path, "the table holds no localization"));
    }
    Ok(Some(survey))
}

fn parse(path: &Path, text: &str) -> Result<Columns, Error> {
    let text = text.trim_start_matches('\u{feff}');
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

fn scales(columns: &Columns, level: Level, peaks: [f64; 3]) -> Scales {
    let extent = level.world_extent();
    let voxel = level.voxel();
    let fallback = [0, 1, 2].map(|axis| 0.5 * voxel[axis].max(f32::MIN_POSITIVE));
    let space = [0, 1, 2].map(|axis| {
        let step = f64::from(voxel[axis]);
        match columns.unit(columns.space(Space::ALL[axis])) {
            Some(unit) => step_of(unit, step),
            None => infer(step, f64::from(extent[axis]), peaks[axis]),
        }
    });
    let sigma = [0, 1, 2].map(|axis| {
        let step = f64::from(voxel[axis]);
        match columns.unit(columns.sigma(Space::ALL[axis])) {
            Some(unit) => step_of(unit, step),
            None => space[axis],
        }
    });
    Scales {
        space,
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

fn row(
    path: &Path,
    columns: &Columns,
    scales: &Scales,
    fields: &[&str],
    line: usize,
    at: u64,
) -> Result<Row, Error> {
    let mut center = [0.0f32; 3];
    for space in Space::ALL {
        let Some(column) = columns.space(space) else {
            continue;
        };
        let axis = space.index();
        let value = number(path, fields, column, line)?;
        center[axis] = narrow(path, value * scales.space[axis], at)?;
    }
    let mut sigma = scales.fallback;
    for space in Space::ALL {
        let axis = space.index();
        let Some(column) = columns.sigma(space) else {
            continue;
        };
        let value = number(path, fields, column, line)?;
        if value > 0.0 {
            sigma[axis] = narrow(path, value * scales.sigma[axis], at)?;
        }
    }
    let quality = match columns.quality {
        Some(column) => narrow(path, number(path, fields, column, line)?, at)?,
        None => 0.0,
    };
    let time = match columns.frame {
        Some(column) => frame(path, number(path, fields, column, line)?, at)?,
        None => 0,
    };
    let group = match columns.group {
        Some(column) => group(path, number(path, fields, column, line)?, at)?,
        None => None,
    };
    Ok(Row {
        time,
        center,
        sigma,
        quality,
        group,
    })
}

fn group(path: &Path, value: f64, row: u64) -> Result<Option<u32>, Error> {
    if value < 0.0 {
        return Ok(None);
    }
    if (value - value.round()).abs() > FRAME_TOLERANCE {
        return Err(Error::table(
            path,
            format!("the row {row} carries the group {value}"),
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
