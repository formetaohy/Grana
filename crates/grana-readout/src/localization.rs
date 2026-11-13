use std::path::{Path, PathBuf};

use grana_volume::Level;

use crate::error::Error;

mod picasso;
mod text;
mod write;

pub use write::{Metadata, write};

const BLOCK: usize = 1 << 16;
const FRAME_TOLERANCE: f64 = 1.0e-6;

#[derive(Clone, Copy)]
pub struct Localization {
    time: u32,
    center: [f32; 3],
    sigma: [f32; 3],
    quality: f32,
    group: Option<u32>,
}

impl Localization {
    pub fn new(
        time: u32,
        center: [f32; 3],
        sigma: [f32; 3],
        quality: f32,
        group: Option<u32>,
    ) -> Self {
        Self {
            time,
            center,
            sigma,
            quality,
            group,
        }
    }

    pub fn time(&self) -> u32 {
        self.time
    }

    pub fn center(&self) -> [f32; 3] {
        self.center
    }

    pub fn sigma(&self) -> [f32; 3] {
        self.sigma
    }

    pub fn quality(&self) -> f32 {
        self.quality
    }

    pub fn group(&self) -> Option<u32> {
        self.group
    }
}

pub struct Frame<'a> {
    time: u32,
    rows: &'a [Localization],
}

impl<'a> Frame<'a> {
    pub fn time(&self) -> u32 {
        self.time
    }

    pub fn rows(&self) -> &'a [Localization] {
        self.rows
    }
}

pub struct Reader {
    path: PathBuf,
    blocks: Blocks,
    place: Place,
    pixel: f32,
    width: u32,
    height: u32,
    volume: bool,
    photons: bool,
    total: u64,
    read: u64,
    buffer: Vec<Localization>,
    start: usize,
    last: Option<u32>,
    eof: bool,
}

enum Blocks {
    Picasso(picasso::Blocks),
    Text(text::Blocks),
}

impl Blocks {
    fn read(&mut self, place: &Place, out: &mut Vec<Localization>) -> Result<usize, Error> {
        match self {
            Self::Picasso(blocks) => blocks.read(place, out),
            Self::Text(blocks) => blocks.read(place, out),
        }
    }
}

pub(super) struct Header {
    pub(super) rows: u64,
    pub(super) first: u32,
    pub(super) last: u32,
    pub(super) pixel: f32,
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) volume: bool,
    pub(super) photons: bool,
}

#[derive(Clone, Copy)]
pub(super) struct Place {
    level: Level,
    shift: u32,
}

pub(super) struct Row {
    pub(super) time: u32,
    pub(super) center: [f32; 3],
    pub(super) sigma: [f32; 3],
    pub(super) quality: f32,
    pub(super) group: Option<u32>,
}

impl Place {
    fn row(&self, path: &Path, at: u64, row: Row) -> Result<Localization, Error> {
        if !row.center.iter().all(|value| value.is_finite()) {
            return Err(Error::table(
                path,
                format!("the row {at} lies at an infinite center"),
            ));
        }
        if !row.quality.is_finite() {
            return Err(Error::table(
                path,
                format!("the row {at} carries an infinite quality"),
            ));
        }
        if !row
            .sigma
            .iter()
            .all(|value| value.is_finite() && *value > 0.0)
        {
            return Err(Error::table(
                path,
                format!("the row {at} carries a localization without a finite sigma"),
            ));
        }
        let Some(time) = row.time.checked_sub(self.shift) else {
            return Err(Error::table(
                path,
                format!("the row {at} carries the frame {}", row.time),
            ));
        };
        let voxel = self.level.voxel();
        let index = [0, 1, 2].map(|axis| row.center[axis] / voxel[axis]);
        Ok(Localization {
            time,
            center: self.level.corner(index),
            sigma: row.sigma,
            quality: row.quality,
            group: row.group,
        })
    }
}

pub fn has_extension(path: &Path) -> bool {
    text::has_extension(path) || picasso::has_extension(path)
}

pub fn open(
    path: &Path,
    level: Level,
    frames: u32,
    report: &mut dyn FnMut(u64, u64) -> bool,
) -> Result<Option<Reader>, Error> {
    assert!(frames > 0, "a localization table names a time series");
    let opened = if picasso::has_extension(path) {
        let (blocks, header) = picasso::open(path, level, frames)?;
        Some((Blocks::Picasso(blocks), header))
    } else {
        text::open(path, level, frames, report)?
            .map(|(blocks, header)| (Blocks::Text(blocks), header))
    };
    let Some((blocks, header)) = opened else {
        return Ok(None);
    };
    if header.rows == 0 {
        return Err(Error::table(path, "the table holds no localization"));
    }
    let shift = shift_of(path, &header, frames)?;
    Ok(Some(Reader {
        path: path.to_path_buf(),
        blocks,
        place: Place { level, shift },
        pixel: header.pixel,
        width: header.width,
        height: header.height,
        volume: header.volume,
        photons: header.photons,
        total: header.rows,
        read: 0,
        buffer: Vec::with_capacity(BLOCK),
        start: 0,
        last: None,
        eof: false,
    }))
}

impl Reader {
    pub fn pixel(&self) -> f32 {
        self.pixel
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn volume(&self) -> bool {
        self.volume
    }

    pub fn photons(&self) -> bool {
        self.photons
    }

    pub fn rows(&self) -> u64 {
        self.total
    }

    pub fn next(
        &mut self,
        report: &mut dyn FnMut(u64, u64) -> bool,
    ) -> Result<Option<Frame<'_>>, Error> {
        loop {
            if self.start == self.buffer.len() {
                if self.eof {
                    return Ok(None);
                }
                if !self.fill(report)? {
                    return Ok(None);
                }
                continue;
            }
            let time = self.buffer[self.start].time;
            let mut end = self.start;
            while end < self.buffer.len() && self.buffer[end].time == time {
                end += 1;
            }
            if end == self.buffer.len() && !self.eof {
                if !self.fill(report)? {
                    return Ok(None);
                }
                continue;
            }
            let rows = &self.buffer[self.start..end];
            self.start = end;
            return Ok(Some(Frame { time, rows }));
        }
    }

    fn fill(&mut self, report: &mut dyn FnMut(u64, u64) -> bool) -> Result<bool, Error> {
        if self.start > 0 {
            self.buffer.drain(..self.start);
            self.start = 0;
        }
        let before = self.buffer.len();
        let place = self.place;
        let read = self.blocks.read(&place, &mut self.buffer)?;
        for (offset, row) in self.buffer[before..].iter().enumerate() {
            if let Some(last) = self.last
                && row.time < last
            {
                return Err(Error::table(
                    &self.path,
                    format!(
                        "the row {} carries the frame {} after the frame {last}; a localization table orders its rows by frame",
                        self.read + offset as u64,
                        row.time
                    ),
                ));
            }
            self.last = Some(row.time);
        }
        self.read += read as u64;
        if read == 0 {
            self.eof = true;
        }
        Ok(report(self.read, self.total))
    }
}

fn shift_of(path: &Path, header: &Header, frames: u32) -> Result<u32, Error> {
    if header.last < frames {
        return Ok(0);
    }
    if header.first >= 1 && header.last - 1 < frames {
        return Ok(1);
    }
    Err(Error::table(
        path,
        format!(
            "the table carries the frames {} to {} and the dataset holds {frames}",
            header.first, header.last
        ),
    ))
}

pub(super) fn normalize(name: &str) -> String {
    name.chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .map(|character| character.to_ascii_lowercase())
        .collect()
}

pub(super) fn frame(path: &Path, value: f64, at: u64) -> Result<u32, Error> {
    if !(0.0..=f64::from(u32::MAX)).contains(&value)
        || (value - value.round()).abs() > FRAME_TOLERANCE
    {
        return Err(Error::table(
            path,
            format!("the row {at} carries the frame {value}"),
        ));
    }
    Ok(value.round() as u32)
}

pub(super) fn narrow(path: &Path, value: f64, at: u64) -> Result<f32, Error> {
    let narrowed = value as f32;
    if !narrowed.is_finite() {
        return Err(Error::table(
            path,
            format!("the row {at} carries the value {value}"),
        ));
    }
    Ok(narrowed)
}
