mod graph;
mod neighbour;
mod skeleton;
mod spanning;
mod volume;

use std::sync::Arc;

use grana_geometry::{GeometryStore, Mesh, push_sphere, push_tube};
use grana_object::{FilamentSet, IntensityTable, SampleAccumulator};
use grana_volume::{Intensity, Level, Pyramid, Region, VoxelData};

use crate::local::LocalField;
use grana_object::FilamentParams;
use grana_object::Mask;

const MAX_VOXELS: u64 = 1 << 25;
const MAX_STEP: u32 = 32;

pub fn filament_level(pyramid: &Pyramid) -> Option<usize> {
    pyramid.fit_level(Region::none(), MAX_VOXELS)
}

pub(super) struct Grid {
    pub dims: [u32; 3],
    pub padded: [u32; 3],
    pub stride: [usize; 3],
    pub placement: Level,
    pub spacing: [f32; 3],
    pub extent: [u32; 3],
    pub step: [u32; 3],
}

impl Grid {
    pub(super) fn position(&self, voxel: [f32; 3]) -> [f32; 3] {
        self.placement
            .corner([0, 1, 2].map(|axis| (voxel[axis] + 0.5) * self.step[axis] as f32))
    }

    pub(super) fn mean_spacing(&self) -> f32 {
        (self.spacing[0] + self.spacing[1] + self.spacing[2]) / 3.0
    }

    pub(super) fn voxel(&self, index: usize) -> [f32; 3] {
        let width = self.padded[0] as usize;
        let depth = self.padded[0] as usize * self.padded[1] as usize;
        [
            (index % width) as f32 - 1.0,
            ((index / width) % self.padded[1] as usize) as f32 - 1.0,
            (index / depth) as f32 - 1.0,
        ]
    }
}

pub struct FilamentScan {
    level: usize,
    extent: [u32; 3],
    step: [u32; 3],
    params: FilamentParams,
    intensity: Intensity,
    threshold: f32,
    channels: usize,
    detection: usize,
    grid: Grid,
    region: Region,
    empty: bool,
    mask: Vec<u8>,
    distance: Vec<u16>,
}

impl FilamentScan {
    pub fn new(
        level: usize,
        info: Level,
        params: FilamentParams,
        channels: usize,
        detection: usize,
        region: Region,
        intensity: Intensity,
    ) -> Self {
        assert!(
            channels > 0 && detection < channels,
            "the detection channel is one of the measured channels"
        );
        let empty = region.voxel_range(info).is_none();
        let extent = info.extent();
        let extent = [extent.x(), extent.y(), extent.z()];
        let spacing = info.spacing();
        let spacing = [spacing.x(), spacing.y(), spacing.z()];
        assert!(
            spacing
                .iter()
                .all(|value| value.is_finite() && *value > 0.0),
            "a filament scan needs a positive voxel size"
        );
        let maximum = spacing.iter().copied().fold(0.0f32, f32::max);
        let step = spacing.map(|value| ((maximum / value).round() as u32).clamp(1, MAX_STEP));
        let working = [0, 1, 2].map(|axis| spacing[axis] * step[axis] as f32);
        let dims = [0, 1, 2].map(|axis| extent[axis].div_ceil(step[axis]));
        assert!(
            dims.iter().all(|value| *value >= 3),
            "the dataset is too small to trace filaments"
        );
        let padded = dims.map(|value| value + 2);
        let voxel_count = padded
            .iter()
            .map(|value| u64::from(*value))
            .product::<u64>();
        assert!(
            voxel_count <= MAX_VOXELS,
            "a filament scan holds at most {MAX_VOXELS} voxels"
        );
        let stride = [
            1,
            padded[0] as usize,
            padded[0] as usize * padded[1] as usize,
        ];
        Self {
            level,
            extent,
            step,
            params,
            intensity,
            threshold: params.threshold(),
            channels,
            detection,
            grid: Grid {
                dims,
                padded,
                stride,
                placement: info,
                spacing: working,
                extent,
                step,
            },
            region,
            empty,
            mask: vec![0; voxel_count as usize],
            distance: vec![0; voxel_count as usize],
        }
    }

    pub fn frame(
        &mut self,
        detection: &VoxelData,
        roi: Option<&Mask>,
        store: &Arc<GeometryStore>,
        mut read: impl FnMut(usize) -> Result<VoxelData, String>,
    ) -> Result<FilamentSet, String> {
        if self.empty {
            return Ok(FilamentSet::new(
                self.level,
                Mesh::new(Vec::new(), Vec::new()),
                Vec::new(),
                IntensityTable::empty(self.channels),
            ));
        }
        let expected = self
            .extent
            .iter()
            .map(|value| u64::from(*value))
            .product::<u64>();
        if detection.voxel_count() as u64 != expected {
            return Err("the detection frame does not cover the requested level".to_string());
        }
        let field = LocalField::new(
            [0, 1, 2].map(|axis| self.extent[axis] as usize),
            [0, 0, 0],
            [0, 0, 0],
            self.intensity,
            1,
            |index| detection.value(index),
            |_| true,
        );
        volume::mask(
            detection,
            &self.grid,
            &field,
            self.threshold,
            volume::Gate {
                region: self.region,
                roi,
            },
            &mut self.mask,
        );
        volume::distance(&self.mask, &self.grid, &mut self.distance);
        skeleton::thin(&mut self.mask, &self.distance, &self.grid);
        let traced = graph::trace(
            &self.mask,
            &self.distance,
            &self.grid,
            self.params.min_length(),
        );
        let mut accumulators = vec![SampleAccumulator::new(); traced.len() * self.channels];
        for channel in 0..self.channels {
            let measured = if channel == self.detection {
                None
            } else {
                Some(read(channel)?)
            };
            let data = match &measured {
                Some(data) => {
                    if data.voxel_count() as u64 != expected {
                        return Err(
                            "the measured frame does not cover the requested level".to_string()
                        );
                    }
                    data
                }
                None => detection,
            };
            for (index, filament) in traced.iter().enumerate() {
                for node in &filament.nodes {
                    accumulators[index * self.channels + channel].push(self.sample(data, *node));
                }
            }
        }
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        for (index, filament) in traced.iter().enumerate() {
            let object = index as u32;
            for path in &filament.paths {
                let points = path.iter().map(|point| point.position).collect::<Vec<_>>();
                let radii = path.iter().map(|point| point.radius).collect::<Vec<_>>();
                push_tube(&mut vertices, &mut indices, &points, &radii, object);
            }
            for joint in &filament.joints {
                push_sphere(
                    &mut vertices,
                    &mut indices,
                    joint.position,
                    joint.radius,
                    object,
                );
            }
        }
        let objects = traced
            .into_iter()
            .map(|filament| filament.statistics)
            .collect::<Vec<_>>();
        let intensities = IntensityTable::new(
            self.channels,
            accumulators
                .into_iter()
                .map(SampleAccumulator::statistics)
                .collect(),
        );
        FilamentSet::paged(
            self.level,
            store,
            Mesh::new(vertices, indices),
            objects,
            intensities,
        )
        .map_err(|error| error.to_string())
    }

    fn sample(&self, data: &VoxelData, node: usize) -> f32 {
        let voxel = self.grid.voxel(node);
        let extent = self.extent;
        let step = self.step;
        let width = extent[0] as usize;
        let height = extent[1] as usize;
        let low = [0, 1, 2].map(|axis| voxel[axis] as u32 * step[axis]);
        let high = [0, 1, 2].map(|axis| ((voxel[axis] as u32 + 1) * step[axis]).min(extent[axis]));
        let mut maximum = f32::NEG_INFINITY;
        for z in low[2]..high[2] {
            for y in low[1]..high[1] {
                let base = width * (z as usize * height + y as usize);
                for x in low[0]..high[0] {
                    maximum = maximum.max(data.value(base + x as usize));
                }
            }
        }
        maximum
    }
}
