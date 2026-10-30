use std::ops::Range;

use grana_volume::{Extent, Spacing};
use ome_zarr_metadata::v0_4::{Axis, AxisType, AxisUnit, AxisUnitSpace};

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
    pub extent: Extent,
    pub spacing: Spacing,
}

impl Layout {
    pub fn channel_count(&self) -> u64 {
        self.channel.map_or(1, |axis| self.shape[axis])
    }

    pub fn ranges(&self, channel: u64) -> Vec<Range<u64>> {
        (0..self.shape.len())
            .map(|axis| {
                if Some(axis) == self.time {
                    0..1
                } else if Some(axis) == self.channel {
                    channel..channel + 1
                } else {
                    0..self.shape[axis]
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
    if let Some(time) = time
        && shape[time] != 1
    {
        return Err(format!(
            "time series with {} frames are not supported",
            shape[time]
        ));
    }

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

    Ok(Layout {
        shape: shape.to_vec(),
        time,
        channel,
        extent,
        spacing: Spacing::new(spacing_x, spacing_y, spacing_z),
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
