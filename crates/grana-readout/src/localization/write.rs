use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use hdf5_pure::{CompoundTypeBuilder, Datatype, FileBuilder};

use crate::error::Error;

const NANOMETER: f32 = 0.001;

const UNGROUPED: i64 = -1;

pub struct Metadata {
    frames: u32,
    width: u32,
    height: u32,
    pixel: f32,
    volume: bool,
    origin: [f32; 3],
}

impl Metadata {
    pub fn new(
        frames: u32,
        width: u32,
        height: u32,
        pixel: f32,
        volume: bool,
        origin: [f32; 3],
    ) -> Self {
        assert!(frames > 0, "a localization table covers a frame");
        assert!(
            width > 0 && height > 0,
            "a localization table covers a positive camera field"
        );
        assert!(
            pixel.is_finite() && pixel > 0.0,
            "a localization table carries a positive camera pixel"
        );
        assert!(
            origin.iter().all(|value| value.is_finite()),
            "a localization table starts at a finite world origin"
        );
        Self {
            frames,
            width,
            height,
            pixel,
            volume,
            origin,
        }
    }
}

struct Group {
    localizations: u64,
    events: u64,
    length: f64,
    square: f64,
    run: f64,
    last: Option<u32>,
}

impl Group {
    fn new() -> Self {
        Self {
            localizations: 0,
            events: 0,
            length: 0.0,
            square: 0.0,
            run: 0.0,
            last: None,
        }
    }

    fn push(&mut self, time: u32) {
        self.localizations += 1;
        match self.last {
            Some(last) if time == last + 1 => self.run += 1.0,
            Some(_) => {
                self.close();
                self.run = 1.0;
            }
            None => self.run = 1.0,
        }
        self.last = Some(time);
    }

    fn close(&mut self) {
        self.events += 1;
        self.length += self.run;
        self.square += self.run * self.run;
    }

    fn summary(&self, path: &Path, id: i64) -> Result<Summary, Error> {
        let count = self.events as f64;
        let length = self.length / count;
        let spread = (self.square / count - length * length).sqrt();
        if !length.is_finite() || !spread.is_finite() {
            return Err(Error::write(path, "a group carries no finite length"));
        }
        Ok(Summary {
            group: id,
            locs: self.localizations,
            events: self.events,
            length,
            spread,
        })
    }
}

struct Summary {
    group: i64,
    locs: u64,
    events: u64,
    length: f64,
    spread: f64,
}

pub struct Table {
    path: PathBuf,
    metadata: Metadata,
    datatype: Datatype,
    encoded: Vec<u8>,
    groups: BTreeMap<i64, Group>,
    reach: [f32; 2],
    rows: u64,
}

impl Table {
    pub fn new(path: &Path, metadata: Metadata) -> Result<Self, Error> {
        let datatype = if metadata.volume {
            volume_datatype()
        } else {
            plane_datatype()
        }
        .map_err(|error| Error::write(path, error.to_string()))?;
        Ok(Self {
            path: path.to_path_buf(),
            metadata,
            datatype,
            encoded: Vec::new(),
            groups: BTreeMap::new(),
            reach: [f32::NEG_INFINITY; 2],
            rows: 0,
        })
    }

    pub fn push(
        &mut self,
        time: u32,
        center: [f32; 3],
        sigma: [f32; 3],
        quality: f32,
        group: Option<u32>,
    ) -> Result<(), Error> {
        if !center.iter().all(|value| value.is_finite()) {
            return Err(Error::write(
                &self.path,
                "a localization lies at an infinite center",
            ));
        }
        if !sigma.iter().all(|value| value.is_finite() && *value > 0.0) {
            return Err(Error::write(
                &self.path,
                "a localization carries a non-positive precision",
            ));
        }
        let pixel = self.metadata.pixel * NANOMETER;
        let origin = self.metadata.origin;
        let x = (center[0] - origin[0]) / pixel;
        let y = (center[1] - origin[1]) / pixel;
        let z = (center[2] - origin[2]) / NANOMETER;
        let sx = sigma[0] / pixel;
        let sy = sigma[1] / pixel;
        let sz = sigma[2] / NANOMETER;
        self.reach[0] = self.reach[0].max(x);
        self.reach[1] = self.reach[1].max(y);
        self.encoded.extend_from_slice(&time.to_le_bytes());
        self.encoded.extend_from_slice(&x.to_le_bytes());
        self.encoded.extend_from_slice(&y.to_le_bytes());
        if self.metadata.volume {
            self.encoded.extend_from_slice(&z.to_le_bytes());
        }
        for value in [quality, sx, sy, sx, sy] {
            self.encoded.extend_from_slice(&value.to_le_bytes());
        }
        if self.metadata.volume {
            self.encoded.extend_from_slice(&sz.to_le_bytes());
        }
        let named = group.map_or(UNGROUPED, i64::from);
        self.encoded.extend_from_slice(&named.to_le_bytes());
        self.rows += 1;
        if group.is_some() {
            self.groups
                .entry(named)
                .or_insert_with(Group::new)
                .push(time);
        }
        Ok(())
    }

    pub fn write(mut self) -> Result<(), Error> {
        if self.rows == 0 {
            return Err(Error::write(
                &self.path,
                "the localization table holds no localization",
            ));
        }
        let groups = self.summaries()?;
        let width = reach(self.reach[0], self.metadata.width);
        let height = reach(self.reach[1], self.metadata.height);
        let Table {
            path,
            metadata,
            datatype,
            encoded,
            rows,
            ..
        } = self;
        let mut builder = FileBuilder::new();
        builder
            .create_dataset("locs")
            .with_compound_data(datatype, encoded, rows);
        if !groups.is_empty() {
            write_groups(&mut builder, &path, &groups)?;
        }
        write_metadata(&mut builder, &metadata, width, height)?;
        let file = grana_file::Atomic::file(&path)
            .map_err(|error| Error::write(&path, error.to_string()))?;
        builder
            .write(file.staging())
            .map_err(|error| Error::write(&path, error.to_string()))?;
        file.commit()
            .map_err(|error| Error::write(&path, error.to_string()))
    }

    fn summaries(&mut self) -> Result<Vec<Summary>, Error> {
        let path = self.path.clone();
        let mut summaries = Vec::with_capacity(self.groups.len());
        for (id, group) in &mut self.groups {
            group.close();
            summaries.push(group.summary(&path, *id)?);
        }
        Ok(summaries)
    }
}

fn write_groups(builder: &mut FileBuilder, path: &Path, groups: &[Summary]) -> Result<(), Error> {
    let datatype = CompoundTypeBuilder::new()
        .i64_field("group")
        .u64_field("locs")
        .u64_field("n_events")
        .f64_field("len_mean")
        .f64_field("len_std")
        .build()
        .map_err(|error| Error::write(path, error.to_string()))?;
    let mut raw = Vec::with_capacity(groups.len() * 40);
    for group in groups {
        raw.extend_from_slice(&group.group.to_le_bytes());
        raw.extend_from_slice(&group.locs.to_le_bytes());
        raw.extend_from_slice(&group.events.to_le_bytes());
        raw.extend_from_slice(&group.length.to_le_bytes());
        raw.extend_from_slice(&group.spread.to_le_bytes());
    }
    builder
        .create_dataset("groups")
        .with_compound_data(datatype, raw, groups.len() as u64);
    Ok(())
}

fn reach(highest: f32, held: u32) -> u32 {
    let highest = highest.max(0.0).floor();
    if highest < held as f32 {
        return held;
    }
    if highest >= u32::MAX as f32 - 1.0 {
        return u32::MAX;
    }
    highest as u32 + 1
}

fn write_metadata(
    builder: &mut FileBuilder,
    metadata: &Metadata,
    width: u32,
    height: u32,
) -> Result<(), Error> {
    let json = format!(
        r#"[{{"Generated by": "Grana {version}"}}, {{"Pixelsize": {pixel}, "Width": {width}, "Height": {height}, "Frames": {frames}}}]"#,
        version = env!("CARGO_PKG_VERSION"),
        pixel = metadata.pixel,
        width = width,
        height = height,
        frames = metadata.frames,
    );
    builder
        .create_dataset("metadata")
        .with_vlen_strings(&[json.as_str()])
        .with_shape(&[]);
    Ok(())
}

fn plane_datatype() -> Result<Datatype, hdf5_pure::FormatError> {
    CompoundTypeBuilder::new()
        .u32_field("frame")
        .f32_field("x")
        .f32_field("y")
        .f32_field("photons")
        .f32_field("sx")
        .f32_field("sy")
        .f32_field("lpx")
        .f32_field("lpy")
        .i64_field("group")
        .build()
}

fn volume_datatype() -> Result<Datatype, hdf5_pure::FormatError> {
    CompoundTypeBuilder::new()
        .u32_field("frame")
        .f32_field("x")
        .f32_field("y")
        .f32_field("z")
        .f32_field("photons")
        .f32_field("sx")
        .f32_field("sy")
        .f32_field("lpx")
        .f32_field("lpy")
        .f32_field("lpz")
        .i64_field("group")
        .build()
}
