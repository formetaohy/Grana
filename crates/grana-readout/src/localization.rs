use std::path::Path;

use grana_volume::Level;

use crate::error::Error;

mod picasso;
mod text;
mod write;

pub use write::{Metadata, write};

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

pub struct Table {
    localizations: Vec<Localization>,
    pixel: f32,
    width: u32,
    height: u32,
    volume: bool,
}

impl Table {
    fn new(
        localizations: Vec<Localization>,
        pixel: f32,
        width: u32,
        height: u32,
        volume: bool,
    ) -> Self {
        assert!(
            pixel.is_finite() && pixel > 0.0,
            "a localization table carries a positive camera pixel"
        );
        assert!(
            width > 0 && height > 0,
            "a localization table covers a positive camera field"
        );
        Self {
            localizations,
            pixel,
            width,
            height,
            volume,
        }
    }

    pub fn localizations(&self) -> &[Localization] {
        &self.localizations
    }

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
}

pub fn has_extension(path: &Path) -> bool {
    text::has_extension(path) || picasso::has_extension(path)
}

pub fn read(
    path: &Path,
    level: Level,
    frames: u32,
    report: &mut dyn FnMut(u64, u64) -> bool,
) -> Result<Option<Table>, Error> {
    assert!(frames > 0, "a localization table names a time series");
    let found = if picasso::has_extension(path) {
        picasso::read(path, level, frames, report)?
    } else {
        text::read(path, level, frames, report)?
    };
    let Some(mut table) = found else {
        return Ok(None);
    };
    if table.localizations.is_empty() {
        return Err(Error::table(path, "the table holds no localization"));
    }
    place(path, &mut table.localizations, level, frames)?;
    Ok(Some(table))
}

fn place(
    path: &Path,
    localizations: &mut [Localization],
    level: Level,
    frames: u32,
) -> Result<(), Error> {
    let mut low = u32::MAX;
    let mut high = 0;
    for localization in localizations.iter() {
        low = low.min(localization.time);
        high = high.max(localization.time);
    }
    let shift = if high < frames {
        0
    } else if low >= 1 && high - 1 < frames {
        1
    } else {
        return Err(Error::table(
            path,
            format!("the table carries the frames {low} to {high} and the dataset holds {frames}"),
        ));
    };
    let voxel = level.voxel();
    for localization in localizations.iter_mut() {
        localization.time -= shift;
        if !localization
            .sigma
            .iter()
            .all(|value| value.is_finite() && *value > 0.0)
        {
            return Err(Error::table(
                path,
                "the table carries a localization without a finite sigma",
            ));
        }
        let index = [0, 1, 2].map(|axis| localization.center[axis] / voxel[axis]);
        localization.center = level.corner(index);
    }
    Ok(())
}

pub(super) fn normalize(name: &str) -> String {
    name.chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .map(|character| character.to_ascii_lowercase())
        .collect()
}

pub(super) fn frame(path: &Path, value: f64, at: usize) -> Result<u32, Error> {
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

pub(super) fn narrow(path: &Path, value: f64, at: usize) -> Result<f32, Error> {
    let narrowed = value as f32;
    if !narrowed.is_finite() {
        return Err(Error::table(
            path,
            format!("the row {at} carries the value {value}"),
        ));
    }
    Ok(narrowed)
}
