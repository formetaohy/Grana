use std::ops::Range;

use grana_volume::Extent;

use crate::metadata::{Axis, AxisKind, Dataset};

#[derive(Clone, Debug)]
pub struct Layout {
    shape: Vec<u64>,
    time: Option<usize>,
    channel: Option<usize>,
    space: [Option<usize>; 3],
    pub extent: Extent,
    pub columns: [[f64; 3]; 3],
    pub origin: [f64; 3],
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

    pub fn voxel(&self) -> [f64; 3] {
        [0, 1, 2].map(|axis| {
            self.columns[axis]
                .iter()
                .map(|value| value * value)
                .sum::<f64>()
                .sqrt()
        })
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
        let spatial = [x, y, z];
        (0..self.shape.len())
            .map(|axis| {
                if Some(axis) == self.time {
                    time..time + 1
                } else if Some(axis) == self.channel {
                    channel..channel + 1
                } else if let Some(index) =
                    (0..3).find(|component| self.space[*component] == Some(axis))
                {
                    let range = &spatial[index];
                    u64::from(range.start)..u64::from(range.end)
                } else {
                    0..1
                }
            })
            .collect()
    }
}

pub fn layout(axes: &[Axis], shape: &[u64], dataset: &Dataset) -> Result<Layout, String> {
    if axes.len() != shape.len() {
        return Err(format!(
            "{} axes describe an array of {} dimensions",
            axes.len(),
            shape.len()
        ));
    }
    let mut time = None;
    let mut channel = None;
    let mut space = [None; 3];
    for (index, axis) in axes.iter().enumerate() {
        match axis.kind() {
            AxisKind::Time => {
                if time.replace(index).is_some() {
                    return Err("the image declares more than one time axis".to_string());
                }
            }
            AxisKind::Channel => {
                if channel.replace(index).is_some() {
                    return Err("the image declares more than one channel axis".to_string());
                }
            }
            AxisKind::Space => match component(axis.name()) {
                Some(slot) => {
                    if space[slot].replace(index).is_some() {
                        return Err(format!(
                            "the image names the spatial axis `{}` twice",
                            axis.name()
                        ));
                    }
                }
                None => fixed_axis(shape, index, axis)?,
            },
            AxisKind::Other => fixed_axis(shape, index, axis)?,
        }
    }
    let Some(x) = space[0] else {
        return Err("the image names no spatial axis `x`".to_string());
    };
    let Some(y) = space[1] else {
        return Err("the image names no spatial axis `y`".to_string());
    };
    let extent = Extent::new(
        dimension(shape[x])?,
        dimension(shape[y])?,
        match space[2] {
            Some(z) => dimension(shape[z])?,
            None => 1,
        },
    );
    let frames = match time {
        Some(time) => dimension(shape[time])?,
        None => 1,
    };
    Ok(Layout {
        shape: shape.to_vec(),
        time,
        channel,
        space,
        extent,
        columns: dataset.columns,
        origin: dataset.origin,
        frames,
        frame_interval: frame_interval(axes, dataset, time)?,
    })
}

pub fn component(name: &str) -> Option<usize> {
    match name.to_ascii_lowercase().as_str() {
        "x" => Some(0),
        "y" => Some(1),
        "z" => Some(2),
        _ => None,
    }
}

fn fixed_axis(shape: &[u64], index: usize, axis: &Axis) -> Result<(), String> {
    if shape[index] != 1 {
        return Err(format!(
            "the axis `{}` of length {} is neither spatial, time nor channel",
            axis.name(),
            shape[index]
        ));
    }
    Ok(())
}

fn dimension(length: u64) -> Result<u32, String> {
    u32::try_from(length)
        .ok()
        .filter(|length| *length > 0)
        .ok_or_else(|| format!("the axis length {length} is outside the supported range"))
}

fn frame_interval(axes: &[Axis], dataset: &Dataset, time: Option<usize>) -> Result<f32, String> {
    let Some(time) = time else {
        return Ok(1.0);
    };
    let value = dataset.scale[time] * axes[time].factor();
    if !value.is_finite() || value <= 0.0 {
        return Err("the scale of the time axis is not a positive duration".to_string());
    }
    let value = value as f32;
    if !value.is_finite() || value <= 0.0 {
        return Err("the scale of the time axis is outside the supported range".to_string());
    }
    Ok(value)
}
