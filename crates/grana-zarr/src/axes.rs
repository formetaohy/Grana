use std::ops::Range;

use grana_volume::{Extent, Spacing};

use crate::metadata::{Axis, AxisKind};

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
    pub translation: [f64; 3],
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

pub fn layout(
    axes: &[Axis],
    shape: &[u64],
    scale: &[f64],
    translation: &[f64],
) -> Result<Layout, String> {
    if axes.len() != shape.len() {
        return Err(format!(
            "{} axes describe an array of {} dimensions",
            axes.len(),
            shape.len()
        ));
    }
    if scale.len() != axes.len() || translation.len() != axes.len() {
        return Err(format!(
            "the coordinate transformation of {} components does not describe {} axes",
            scale.len(),
            axes.len()
        ));
    }
    let kinds = axes.iter().map(kind).collect::<Vec<_>>();
    let space_count = kinds.iter().filter(|kind| **kind == Kind::Space).count();
    if !(2..=3).contains(&space_count) {
        return Err(format!(
            "a spatial image needs two or three space axes, found {space_count}"
        ));
    }
    let time = single(&kinds, Kind::Time, "time")?;
    let channel = single(&kinds, Kind::Channel, "channel")?;
    let first_space = axes.len() - space_count;
    let mut expected = 0;
    if time.is_some() {
        if time != Some(0) {
            return Err("the time axis must precede the channel and space axes".to_string());
        }
        expected = 1;
    }
    if channel.is_some() {
        if channel != Some(expected) {
            return Err("the channel axis must follow the time axis".to_string());
        }
        expected += 1;
    }
    if first_space != expected {
        return Err("time and channel axes must precede the space axes".to_string());
    }
    if !kinds[first_space..].iter().all(|kind| *kind == Kind::Space) {
        return Err("the space axes must be the trailing axes".to_string());
    }

    let x = axes.len() - 1;
    let y = axes.len() - 2;
    let z = (space_count == 3).then_some(axes.len() - 3);
    let mut expected_names = vec!["y", "x"];
    if z.is_some() {
        expected_names.insert(0, "z");
    }
    let names = axes[first_space..]
        .iter()
        .map(|axis| axis.name().to_ascii_lowercase())
        .collect::<Vec<_>>();
    if names != expected_names {
        return Err(format!(
            "the space axes `{}` must be ordered z, y, x",
            names.join(", ")
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

    let spacing_x = spacing(&axes[x], scale[x])?;
    let spacing_y = spacing(&axes[y], scale[y])?;
    let spacing_z = match z {
        Some(z) => spacing(&axes[z], scale[z])?,
        None => spacing_x.min(spacing_y),
    };
    let translation_x = length(&axes[x], translation[x])?;
    let translation_y = length(&axes[y], translation[y])?;
    let translation_z = match z {
        Some(z) => length(&axes[z], translation[z])?,
        None => 0.0,
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
        translation: [translation_x, translation_y, translation_z],
        frames,
        frame_interval: frame_interval(scale, time)?,
    })
}

fn kind(axis: &Axis) -> Kind {
    match axis.kind() {
        AxisKind::Space => Kind::Space,
        AxisKind::Time => Kind::Time,
        AxisKind::Channel => Kind::Channel,
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

fn spacing(axis: &Axis, scale: f64) -> Result<f32, String> {
    if !scale.is_finite() || scale <= 0.0 {
        return Err(format!(
            "the scale of the axis `{}` is not a positive length",
            axis.name()
        ));
    }
    let value = scale as f32;
    if !value.is_finite() || value <= 0.0 {
        return Err(format!(
            "the scale of the axis `{}` is outside the supported range",
            axis.name()
        ));
    }
    Ok(value)
}

fn length(axis: &Axis, translation: f64) -> Result<f64, String> {
    if !translation.is_finite() {
        return Err(format!(
            "the translation of the axis `{}` is outside the supported range",
            axis.name()
        ));
    }
    Ok(translation)
}

fn frame_interval(scale: &[f64], time: Option<usize>) -> Result<f32, String> {
    let Some(time) = time else {
        return Ok(1.0);
    };
    let value = scale[time];
    if !value.is_finite() || value <= 0.0 {
        return Err("the scale of the time axis is not a positive duration".to_string());
    }
    let value = value as f32;
    if !value.is_finite() || value <= 0.0 {
        return Err("the scale of the time axis is outside the supported range".to_string());
    }
    Ok(value)
}
