use std::ops::Range;

use grana_volume::{Extent, Filter, Level, VoxelData};

use crate::Error;
use crate::image::GroupImage;

use super::graph::Spatial;
use super::grid::{apply, basis_matrix, dataset_columns, invert, multiply};

pub(super) struct Member {
    pub(super) group: String,
    pub(super) image: GroupImage,
    pub(super) spatial: Spatial,
    pub(super) geometry: Vec<Geometry>,
    pub(super) placements: Vec<Reindex>,
    pub(super) frames: u32,
}

pub(super) struct Geometry {
    pub(super) columns: [[f64; 3]; 3],
    pub(super) origin: [f64; 3],
    pub(super) extent: Extent,
}

pub(super) struct Reindex {
    pub(super) linear: [[f64; 3]; 3],
    pub(super) offset: [f64; 3],
}

impl Member {
    pub(super) fn new(
        tag: &str,
        group: String,
        image: GroupImage,
        spatial: Spatial,
    ) -> Result<Self, Error> {
        let frames = image.pyramid().frame_count();
        if invert(spatial.linear).is_none() {
            return Err(Error::Unsupported {
                path: tag.to_string(),
                detail: format!("the placement of `{group}` is not invertible"),
            });
        }
        if image.datasets().len() != image.pyramid().level_count() {
            return Err(Error::Unsupported {
                path: tag.to_string(),
                detail: format!("the group `{group}` does not describe the scales of its levels"),
            });
        }
        let geometry = image
            .datasets()
            .iter()
            .zip(image.pyramid().levels())
            .map(|(dataset, level)| Geometry {
                columns: dataset_columns(dataset),
                origin: dataset.origin,
                extent: level.extent(),
            })
            .collect::<Vec<_>>();
        Ok(Self {
            group,
            image,
            spatial,
            geometry,
            placements: Vec::new(),
            frames,
        })
    }

    pub(super) fn index(
        &mut self,
        tag: &str,
        levels: &[Level],
        shift: [f64; 3],
    ) -> Result<(), Error> {
        let forward = invert(self.spatial.linear).ok_or_else(|| Error::Unsupported {
            path: tag.to_string(),
            detail: format!("the placement of `{}` is not invertible", self.group),
        })?;
        let mut resolved = Vec::with_capacity(levels.len());
        for (composite, member) in levels.iter().zip(&self.geometry) {
            let inverse = invert(member.columns).ok_or_else(|| Error::Unsupported {
                path: tag.to_string(),
                detail: format!("the placement of `{}` is not invertible", self.group),
            })?;
            let basis = basis_matrix(composite.basis());
            let reverse = invert(basis).ok_or_else(|| Error::Unsupported {
                path: tag.to_string(),
                detail: "the composite grid is not invertible".to_string(),
            })?;
            let linear = multiply(multiply(inverse, forward), basis);
            let corner = apply(self.spatial.linear, self.spatial.offset, member.origin);
            let frame = [0usize, 1, 2]
                .map(|component| composite.origin()[component] as f64 + shift[component]);
            let within = apply(
                reverse,
                [0.0; 3],
                [
                    corner[0] - frame[0],
                    corner[1] - frame[1],
                    corner[2] - frame[2],
                ],
            );
            let mut offset = [0.0; 3];
            for component in 0..3 {
                offset[component] = 0.5
                    * (linear[component][0] + linear[component][1] + linear[component][2] - 1.0)
                    - linear[component][0] * within[0]
                    - linear[component][1] * within[1]
                    - linear[component][2] * within[2];
            }
            resolved.push(Reindex { linear, offset });
        }
        self.placements = resolved;
        Ok(())
    }

    pub(super) fn time(&self, time: u32) -> u32 {
        if self.frames == 1 { 0 } else { time }
    }

    pub(super) fn coverage(
        &self,
        level: usize,
        ranges: &[Range<u32>; 3],
    ) -> Option<[Range<u32>; 3]> {
        let reindex = self.placements.get(level)?;
        reindex.coverage(ranges, self.geometry.get(level)?.extent)
    }

    pub(super) fn paint(
        &self,
        output: &mut VoxelData,
        ranges: &[Range<u32>; 3],
        level: usize,
        time: u32,
        channel: usize,
    ) -> Result<(), Error> {
        let Some(reindex) = self.placements.get(level) else {
            return Ok(());
        };
        let Some(source) = self.coverage(level, ranges) else {
            return Ok(());
        };
        let data = self.image.read_region(
            self.time(time),
            level,
            channel,
            source[0].clone(),
            source[1].clone(),
            source[2].clone(),
        )?;
        let dims = [
            (source[0].end - source[0].start) as usize,
            (source[1].end - source[1].start) as usize,
            (source[2].end - source[2].start) as usize,
        ];
        if dims.contains(&0) {
            return Ok(());
        }
        let filter = if self
            .image
            .pyramid()
            .channels()
            .get(channel)
            .is_none_or(|info| info.semantics().averages())
        {
            Filter::Linear
        } else {
            Filter::Nearest
        };
        let base = reindex.at([
            ranges[0].start as f64,
            ranges[1].start as f64,
            ranges[2].start as f64,
        ]);
        let first = [
            base[0] - source[0].start as f64,
            base[1] - source[1].start as f64,
            base[2] - source[2].start as f64,
        ];
        let column = |axis: usize| {
            [
                reindex.linear[0][axis],
                reindex.linear[1][axis],
                reindex.linear[2][axis],
            ]
        };
        let step = [column(0), column(1), column(2)];
        let width = (ranges[0].end - ranges[0].start) as usize;
        let height = (ranges[1].end - ranges[1].start) as usize;
        let depth = (ranges[2].end - ranges[2].start) as usize;
        let stride = [
            1usize,
            (ranges[0].end - ranges[0].start) as usize,
            (ranges[0].end - ranges[0].start) as usize * (ranges[1].end - ranges[1].start) as usize,
        ];
        for z in 0..depth {
            for y in 0..height {
                let mut position = [
                    first[0] + step[2][0] * z as f64 + step[1][0] * y as f64,
                    first[1] + step[2][1] * z as f64 + step[1][1] * y as f64,
                    first[2] + step[2][2] * z as f64 + step[1][2] * y as f64,
                ];
                let row = (ranges[2].start as usize + z) * stride[2]
                    + (ranges[1].start as usize + y) * stride[1]
                    + ranges[0].start as usize;
                for x in 0..width {
                    if let Some(value) = data.sample(dims, position, filter) {
                        output.set_value(row + x, value);
                    }
                    position[0] += step[0][0];
                    position[1] += step[0][1];
                    position[2] += step[0][2];
                }
            }
        }
        Ok(())
    }
}

impl Reindex {
    fn at(&self, index: [f64; 3]) -> [f64; 3] {
        apply(self.linear, self.offset, index)
    }

    fn coverage(&self, ranges: &[Range<u32>; 3], extent: Extent) -> Option<[Range<u32>; 3]> {
        let limits = [extent.x(), extent.y(), extent.z()];
        let mut low = [f64::INFINITY; 3];
        let mut high = [f64::NEG_INFINITY; 3];
        for corner in 0..8usize {
            let index = [0usize, 1, 2].map(|axis| {
                let end = if corner & (1 << axis) == 0 {
                    ranges[axis].start
                } else {
                    ranges[axis].end
                };
                end as f64
            });
            let at = self.at(index);
            for axis in 0..3 {
                low[axis] = low[axis].min(at[axis]);
                high[axis] = high[axis].max(at[axis]);
            }
        }
        let mut output = [0..0u32, 0..0, 0..0];
        for axis in 0..3 {
            let start = (low[axis].floor() - 1.0).max(0.0);
            let end = (high[axis].ceil() + 1.0).min(f64::from(limits[axis]));
            let start = (start as u32).min(limits[axis]);
            let end = (end as u32).min(limits[axis]);
            if start >= end {
                return None;
            }
            output[axis] = start..end;
        }
        Some(output)
    }
}
