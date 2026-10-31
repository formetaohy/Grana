mod graph;
mod neighbour;
mod skeleton;
mod spanning;
mod volume;

use grana_object::{FilamentSet, IntensityTable, Mesh, SampleAccumulator, push_sphere, push_tube};
use grana_volume::{Level, Pyramid, VoxelData};

use crate::FilamentParams;

const MAX_VOXELS: u64 = 1 << 25;
const MAX_STEP: u32 = 32;

pub fn filament_level(pyramid: &Pyramid) -> Option<usize> {
    (0..pyramid.level_count()).find(|index| voxels(pyramid.level(*index)) <= MAX_VOXELS)
}

fn voxels(level: Level) -> u64 {
    let extent = level.extent();
    u64::from(extent.x()) * u64::from(extent.y()) * u64::from(extent.z())
}

pub(super) struct Grid {
    pub dims: [u32; 3],
    pub padded: [u32; 3],
    pub stride: [usize; 3],
    pub spacing: [f32; 3],
    pub world: [f32; 3],
}

impl Grid {
    pub(super) fn position(&self, voxel: [f32; 3]) -> [f32; 3] {
        [0, 1, 2].map(|axis| (voxel[axis] + 0.5) * self.spacing[axis] - 0.5 * self.world[axis])
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
    channels: usize,
    grid: Grid,
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
    ) -> Self {
        assert!(
            channels > 0 && detection < channels,
            "the detection channel is one of the measured channels"
        );
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
        let world = info.world_extent();
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
            channels,
            grid: Grid {
                dims,
                padded,
                stride,
                spacing: working,
                world,
            },
            mask: vec![0; voxel_count as usize],
            distance: vec![0; voxel_count as usize],
        }
    }

    pub fn frame(
        &mut self,
        detection: &VoxelData,
        mut read: impl FnMut(usize) -> Result<VoxelData, String>,
    ) -> Result<FilamentSet, String> {
        let expected = self
            .extent
            .iter()
            .map(|value| u64::from(*value))
            .product::<u64>();
        if detection.voxel_count() as u64 != expected {
            return Err("the detection frame does not cover the requested level".to_string());
        }
        volume::mask(
            detection,
            self.extent,
            self.step,
            &self.grid,
            self.params.threshold(),
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
            let data = read(channel)?;
            if data.voxel_count() as u64 != expected {
                return Err("the measured frame does not cover the requested level".to_string());
            }
            for (index, filament) in traced.iter().enumerate() {
                for node in &filament.nodes {
                    accumulators[index * self.channels + channel].push(self.sample(&data, *node));
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
        Ok(FilamentSet::new(
            self.level,
            Mesh::new(vertices, indices),
            objects,
            intensities,
        ))
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
