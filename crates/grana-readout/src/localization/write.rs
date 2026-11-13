use std::collections::BTreeMap;
use std::path::Path;

use hdf5_pure::{CompoundTypeBuilder, FileBuilder};

use super::Localization;
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

struct Record {
    frame: u32,
    x: f32,
    y: f32,
    z: f32,
    photons: f32,
    sx: f32,
    sy: f32,
    lpx: f32,
    lpy: f32,
    lpz: f32,
    group: i64,
}

struct GroupRecord {
    group: i64,
    locs: u64,
    events: u64,
    length: f64,
    spread: f64,
}

pub fn write(
    path: &Path,
    metadata: &Metadata,
    localizations: &[Localization],
) -> Result<(), Error> {
    if localizations.is_empty() {
        return Err(Error::write(path, "the object set holds no localization"));
    }
    let pixel = metadata.pixel * NANOMETER;
    let records = localizations
        .iter()
        .map(|entry| record(path, entry, pixel, metadata.origin))
        .collect::<Result<Vec<_>, Error>>()?;
    let groups = groups(path, &records)?;
    let mut builder = FileBuilder::new();
    if metadata.volume {
        write_volume(&mut builder, path, &records)?;
    } else {
        write_plane(&mut builder, path, &records)?;
    }
    if !groups.is_empty() {
        write_groups(&mut builder, path, &groups)?;
    }
    let width = reach(&records, metadata.width, |record| record.x);
    let height = reach(&records, metadata.height, |record| record.y);
    write_metadata(&mut builder, metadata, width, height)?;
    builder
        .write(path)
        .map_err(|error| Error::write(path, error.to_string()))
}

fn record(
    path: &Path,
    entry: &Localization,
    pixel: f32,
    origin: [f32; 3],
) -> Result<Record, Error> {
    let center = entry.center();
    let sigma = entry.sigma();
    if !center.iter().all(|value| value.is_finite()) {
        return Err(Error::write(
            path,
            "a localization lies at an infinite center",
        ));
    }
    if !sigma.iter().all(|value| value.is_finite() && *value > 0.0) {
        return Err(Error::write(
            path,
            "a localization carries a non-positive precision",
        ));
    }
    let lateral = |value: f32| value / pixel;
    Ok(Record {
        frame: entry.time(),
        x: lateral(center[0] - origin[0]),
        y: lateral(center[1] - origin[1]),
        z: (center[2] - origin[2]) / NANOMETER,
        photons: entry.quality(),
        sx: lateral(sigma[0]),
        sy: lateral(sigma[1]),
        lpx: lateral(sigma[0]),
        lpy: lateral(sigma[1]),
        lpz: sigma[2] / NANOMETER,
        group: entry.group().map_or(UNGROUPED, i64::from),
    })
}

fn groups(path: &Path, records: &[Record]) -> Result<Vec<GroupRecord>, Error> {
    let mut held = BTreeMap::<i64, Vec<u32>>::new();
    for record in records {
        if record.group >= 0 {
            held.entry(record.group).or_default().push(record.frame);
        }
    }
    let mut groups = Vec::with_capacity(held.len());
    for (group, mut frames) in held {
        frames.sort_unstable();
        let mut events = 0u64;
        let mut lengths = Vec::new();
        let mut start = 0usize;
        for index in 1..=frames.len() {
            let broken = index == frames.len() || frames[index] != frames[index - 1] + 1;
            if broken {
                events += 1;
                lengths.push((index - start) as f64);
                start = index;
            }
        }
        let count = lengths.len() as f64;
        let length = lengths.iter().sum::<f64>() / count;
        let spread = (lengths
            .iter()
            .map(|value| (value - length) * (value - length))
            .sum::<f64>()
            / count)
            .sqrt();
        if !length.is_finite() || !spread.is_finite() {
            return Err(Error::write(path, "a group carries no finite length"));
        }
        groups.push(GroupRecord {
            group,
            locs: frames.len() as u64,
            events,
            length,
            spread,
        });
    }
    Ok(groups)
}

fn write_plane(builder: &mut FileBuilder, path: &Path, records: &[Record]) -> Result<(), Error> {
    let datatype = plane_datatype().map_err(|error| Error::write(path, error.to_string()))?;
    let mut raw = Vec::with_capacity(records.len() * 40);
    for record in records {
        raw.extend_from_slice(&record.frame.to_le_bytes());
        for value in [
            record.x,
            record.y,
            record.photons,
            record.sx,
            record.sy,
            record.lpx,
            record.lpy,
        ] {
            raw.extend_from_slice(&value.to_le_bytes());
        }
        raw.extend_from_slice(&record.group.to_le_bytes());
    }
    builder
        .create_dataset("locs")
        .with_compound_data(datatype, raw, records.len() as u64);
    Ok(())
}

fn write_volume(builder: &mut FileBuilder, path: &Path, records: &[Record]) -> Result<(), Error> {
    let datatype = volume_datatype().map_err(|error| Error::write(path, error.to_string()))?;
    let mut raw = Vec::with_capacity(records.len() * 48);
    for record in records {
        raw.extend_from_slice(&record.frame.to_le_bytes());
        for value in [
            record.x,
            record.y,
            record.z,
            record.photons,
            record.sx,
            record.sy,
            record.lpx,
            record.lpy,
            record.lpz,
        ] {
            raw.extend_from_slice(&value.to_le_bytes());
        }
        raw.extend_from_slice(&record.group.to_le_bytes());
    }
    builder
        .create_dataset("locs")
        .with_compound_data(datatype, raw, records.len() as u64);
    Ok(())
}

fn write_groups(
    builder: &mut FileBuilder,
    path: &Path,
    groups: &[GroupRecord],
) -> Result<(), Error> {
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

fn reach(records: &[Record], held: u32, axis: impl Fn(&Record) -> f32) -> u32 {
    let highest = records
        .iter()
        .map(axis)
        .filter(|value| value.is_finite())
        .fold(0.0f32, f32::max)
        .floor();
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

fn plane_datatype() -> Result<hdf5_pure::Datatype, hdf5_pure::FormatError> {
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

fn volume_datatype() -> Result<hdf5_pure::Datatype, hdf5_pure::FormatError> {
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
