use std::ops::Range;

use grana_volume::{Extent, Spacing};
use ome_zarr_metadata::v0_4::{Axis, AxisType, AxisUnit, AxisUnitSpace, AxisUnitTime};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Time,
    Channel,
    Space,
}

#[derive(Clone, Debug)]
pub struct Layout {
    shape: Vec<u64>,
    time: Option<usize>,
    channel: Option<usize>,
    space_count: usize,
    pub extent: Extent,
    pub spacing: Spacing,
    frames: u32,
    frame_interval: f32,
}

impl Layout {
    pub fn channel_count(&self) -> u64 {
        self.channel.map_or(1, |axis| self.shape[axis])
    }

    pub fn frame_count(&self) -> u32 {
        self.frames
    }

    pub fn frame_interval(&self) -> f32 {
        self.frame_interval
    }

    pub fn ranges(
        &self,
        time: u64,
        channel: u64,
        x: Range<u32>,
        y: Range<u32>,
        z: Range<u32>,
    ) -> Vec<Range<u64>> {
        assert!(
            time < u64::from(self.frames),
            "the frame lies inside the layout"
        );
        let first_space = self.shape.len() - self.space_count;
        let spatial = if self.space_count == 3 {
            [z, y, x]
        } else {
            [y, x, 0..0]
        };
        (0..self.shape.len())
            .map(|axis| {
                if Some(axis) == self.time {
                    time..time + 1
                } else if Some(axis) == self.channel {
                    channel..channel + 1
                } else {
                    let range = &spatial[axis - first_space];
                    u64::from(range.start)..u64::from(range.end)
                }
            })
            .collect()
    }
}

pub fn layout(axes: &[Axis], shape: &[u64], scale: &[f32]) -> Result<Layout, String> {
    if axes.len() != shape.len() {
        return Err(format!(
            "{} axes describe an array of {} dimensions",
            axes.len(),
            shape.len()
        ));
    }
    if scale.len() != axes.len() {
        return Err(format!(
            "the scale transform has {} components for {} axes",
            scale.len(),
            axes.len()
        ));
    }
    let kinds = axes.iter().map(kind).collect::<Result<Vec<_>, String>>()?;
    let space_count = kinds.iter().filter(|kind| **kind == Kind::Space).count();
    if !(2..=3).contains(&space_count) {
        return Err(format!(
            "a spatial image needs two or three space axes, found {space_count}"
        ));
    }
    let time = single(&kinds, Kind::Time, "time")?;
    let channel = single(&kinds, Kind::Channel, "channel")?;
    let first_space = axes.len() - space_count;
    let order = kinds[..first_space]
        .iter()
        .copied()
        .filter(|kind| *kind != Kind::Space)
        .collect::<Vec<_>>();
    let mut expected = Vec::new();
    if time.is_some() {
        expected.push(Kind::Time);
    }
    if channel.is_some() {
        expected.push(Kind::Channel);
    }
    if order != expected {
        return Err("time and channel axes must precede the space axes".to_string());
    }
    if !kinds[first_space..].iter().all(|kind| *kind == Kind::Space) {
        return Err("the space axes must be the trailing axes".to_string());
    }

    let x = axes.len() - 1;
    let y = axes.len() - 2;
    let z = (space_count == 3).then_some(axes.len() - 3);

    let extent = Extent::new(
        dimension(shape[x])?,
        dimension(shape[y])?,
        match z {
            Some(z) => dimension(shape[z])?,
            None => 1,
        },
    );

    let spacing_x = spacing(axes, scale, x)?;
    let spacing_y = spacing(axes, scale, y)?;
    let spacing_z = match z {
        Some(z) => spacing(axes, scale, z)?,
        None => spacing_x.min(spacing_y),
    };

    let frames = match time {
        Some(time) => dimension(shape[time])?,
        None => 1,
    };

    Ok(Layout {
        shape: shape.to_vec(),
        time,
        channel,
        space_count,
        extent,
        spacing: Spacing::new(spacing_x, spacing_y, spacing_z),
        frames,
        frame_interval: frame_interval(axes, scale, time)?,
    })
}

fn kind(axis: &Axis) -> Result<Kind, String> {
    match &axis.r#type {
        Some(AxisType::Space) => Ok(Kind::Space),
        Some(AxisType::Time) => Ok(Kind::Time),
        Some(AxisType::Channel) => Ok(Kind::Channel),
        Some(AxisType::Custom(name)) => Err(format!("the custom axis `{name}` is not supported")),
        _ => match axis.name.as_str() {
            "x" | "y" | "z" => Ok(Kind::Space),
            "c" => Ok(Kind::Channel),
            "t" => Ok(Kind::Time),
            name => Err(format!("the axis `{name}` has no recognisable type")),
        },
    }
}

fn single(kinds: &[Kind], kind: Kind, name: &str) -> Result<Option<usize>, String> {
    let matches = kinds
        .iter()
        .enumerate()
        .filter(|(_, candidate)| **candidate == kind)
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [] => Ok(None),
        [index] => Ok(Some(*index)),
        _ => Err(format!("the image declares more than one {name} axis")),
    }
}

fn dimension(length: u64) -> Result<u32, String> {
    u32::try_from(length)
        .ok()
        .filter(|length| *length > 0)
        .ok_or_else(|| format!("the axis length {length} is outside the supported range"))
}

fn spacing(axes: &[Axis], scale: &[f32], axis: usize) -> Result<f32, String> {
    let factor = match &axes[axis].unit {
        Some(AxisUnit::Space(unit)) => space_factor(*unit)?,
        Some(AxisUnit::Time(_)) => {
            return Err(format!(
                "the space axis `{}` carries a time unit",
                axes[axis].name
            ));
        }
        Some(AxisUnit::Custom(unit)) => {
            return Err(format!("the unit `{unit}` is not supported"));
        }
        _ => 1.0,
    };
    let value = f64::from(scale[axis]) * factor;
    if !value.is_finite() || value <= 0.0 {
        return Err(format!(
            "the scale of the axis `{}` is not a positive length",
            axes[axis].name
        ));
    }
    let value = value as f32;
    if !value.is_finite() {
        return Err(format!(
            "the scale of the axis `{}` is outside the supported range",
            axes[axis].name
        ));
    }
    Ok(value)
}

fn space_factor(unit: AxisUnitSpace) -> Result<f64, String> {
    let micrometer = match unit {
        AxisUnitSpace::Angstrom => 1e-4,
        AxisUnitSpace::Attometer => 1e-12,
        AxisUnitSpace::Centimeter => 1e4,
        AxisUnitSpace::Decimeter => 1e5,
        AxisUnitSpace::Exameter => 1e24,
        AxisUnitSpace::Femtometer => 1e-9,
        AxisUnitSpace::Foot => 304_800.0,
        AxisUnitSpace::Gigameter => 1e15,
        AxisUnitSpace::Hectometer => 1e8,
        AxisUnitSpace::Inch => 25_400.0,
        AxisUnitSpace::Kilometer => 1e9,
        AxisUnitSpace::Megameter => 1e12,
        AxisUnitSpace::Meter => 1e6,
        AxisUnitSpace::Micrometer => 1.0,
        AxisUnitSpace::Mile => 1_609_344_000.0,
        AxisUnitSpace::Millimeter => 1e3,
        AxisUnitSpace::Nanometer => 1e-3,
        AxisUnitSpace::Parsec => 3.085_677_581_491_367e22,
        AxisUnitSpace::Petameter => 1e21,
        AxisUnitSpace::Picometer => 1e-6,
        AxisUnitSpace::Terameter => 1e18,
        AxisUnitSpace::Yard => 914_400.0,
        AxisUnitSpace::Yoctometer => 1e-18,
        AxisUnitSpace::Yottameter => 1e30,
        AxisUnitSpace::Zeptometer => 1e-15,
        AxisUnitSpace::Zettameter => 1e27,
        _ => return Err("the physical unit is not supported".to_string()),
    };
    Ok(micrometer)
}

fn frame_interval(axes: &[Axis], scale: &[f32], time: Option<usize>) -> Result<f32, String> {
    let Some(time) = time else {
        return Ok(1.0);
    };
    let factor = match &axes[time].unit {
        Some(AxisUnit::Time(unit)) => time_factor(*unit)?,
        Some(AxisUnit::Space(_)) => {
            return Err(format!(
                "the time axis `{}` carries a space unit",
                axes[time].name
            ));
        }
        Some(AxisUnit::Custom(unit)) => {
            return Err(format!("the unit `{unit}` is not supported"));
        }
        None => 1.0,
        Some(_) => return Err("the time unit is not supported".to_string()),
    };
    let value = f64::from(scale[time]) * factor;
    if !value.is_finite() || value <= 0.0 {
        return Err(format!(
            "the scale of the axis `{}` is not a positive duration",
            axes[time].name
        ));
    }
    let value = value as f32;
    if !value.is_finite() {
        return Err(format!(
            "the scale of the axis `{}` is outside the supported range",
            axes[time].name
        ));
    }
    Ok(value)
}

fn time_factor(unit: AxisUnitTime) -> Result<f64, String> {
    let second = match unit {
        AxisUnitTime::Attosecond => 1e-18,
        AxisUnitTime::Centisecond => 1e-2,
        AxisUnitTime::Day => 86_400.0,
        AxisUnitTime::Decisecond => 1e-1,
        AxisUnitTime::Exasecond => 1e18,
        AxisUnitTime::Femtosecond => 1e-15,
        AxisUnitTime::Gigasecond => 1e9,
        AxisUnitTime::Hectosecond => 1e2,
        AxisUnitTime::Hour => 3_600.0,
        AxisUnitTime::Kilosecond => 1e3,
        AxisUnitTime::Megasecond => 1e6,
        AxisUnitTime::Microsecond => 1e-6,
        AxisUnitTime::Millisecond => 1e-3,
        AxisUnitTime::Minute => 60.0,
        AxisUnitTime::Nanosecond => 1e-9,
        AxisUnitTime::Petasecond => 1e15,
        AxisUnitTime::Picosecond => 1e-12,
        AxisUnitTime::Second => 1.0,
        AxisUnitTime::Terasecond => 1e12,
        AxisUnitTime::Yoctosecond => 1e-24,
        AxisUnitTime::Yottasecond => 1e24,
        AxisUnitTime::Zeptosecond => 1e-21,
        AxisUnitTime::Zettasecond => 1e21,
        _ => return Err("the time unit is not supported".to_string()),
    };
    Ok(second)
}
