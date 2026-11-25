mod graph;
mod neighbour;
mod skeleton;
mod spanning;
mod volume;

use std::sync::Arc;

use grana_geometry::Mesh;
use grana_object::{FilamentSet, IntensityTable, ObjectIdSource, ObjectIds, SampleAccumulator};
use grana_store::SpillStore;
use grana_volume::{Intensity, Level, Pyramid, Region, VoxelData};

use crate::budget::Budget;
use crate::channel::ChannelSet;
use crate::local::LocalField;
use grana_object::FilamentParams;
use grana_object::Mask;

const MAX_STEP: u32 = 32;

pub fn filament_level(pyramid: &Pyramid, budget: Budget) -> Option<usize> {
    pyramid.fit_level_by(budget.filament_voxels(), |index| {
        let extent = pyramid.level(index).extent();
        Some(u64::from(extent.x()) * u64::from(extent.y()) * u64::from(extent.z()))
    })
}

pub fn filament_fits(info: Level, budget: Budget) -> bool {
    let (_, dims) = sampling(info);
    let padded = dims.map(|value| value + 2);
    dims.iter().all(|value| *value >= 3)
        && padded
            .iter()
            .map(|value| u64::from(*value))
            .product::<u64>()
            <= budget.filament_voxels()
}

fn sampling(info: Level) -> ([u32; 3], [u32; 3]) {
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
    let dims = [0, 1, 2].map(|axis| extent[axis].div_ceil(step[axis]));
    (step, dims)
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

    pub(super) fn unit(&self) -> f32 {
        self.spacing[0].min(self.spacing[1]).min(self.spacing[2])
    }

    pub(super) fn radius(&self, distance: u16) -> f32 {
        volume::radius(distance, self.unit())
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
    params: FilamentParams,
    intensity: Intensity,
    threshold: f32,
    channels: ChannelSet,
    grid: Grid,
    region: Region,
    empty: bool,
    budget: Budget,
    mask: Vec<u8>,
    tube: Vec<u8>,
    distance: Vec<u16>,
    owner: Vec<u32>,
    reach: Vec<u32>,
}

impl FilamentScan {
    pub fn new(
        level: usize,
        info: Level,
        params: FilamentParams,
        channels: ChannelSet,
        region: Region,
        intensity: Intensity,
        budget: Budget,
    ) -> Self {
        let empty = region.voxel_range(info).is_none();
        let extent = info.extent();
        let extent = [extent.x(), extent.y(), extent.z()];
        let spacing = info.spacing();
        let spacing = [spacing.x(), spacing.y(), spacing.z()];
        let (step, dims) = sampling(info);
        let working = [0, 1, 2].map(|axis| spacing[axis] * step[axis] as f32);
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
            voxel_count <= budget.filament_voxels(),
            "a filament scan holds at most {} voxels",
            budget.filament_voxels()
        );
        let stride = [
            1,
            padded[0] as usize,
            padded[0] as usize * padded[1] as usize,
        ];
        Self {
            level,
            extent,
            params,
            intensity,
            threshold: params.threshold(),
            channels,
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
            budget,
            mask: vec![0; voxel_count as usize],
            tube: vec![0; voxel_count as usize],
            distance: vec![0; voxel_count as usize],
            owner: vec![0; voxel_count as usize],
            reach: vec![0; voxel_count as usize],
        }
    }

    pub fn frame(
        &mut self,
        detection: &VoxelData,
        roi: Option<&Mask>,
        store: &Arc<SpillStore>,
        mut read: impl FnMut(usize) -> Result<VoxelData, String>,
        ids: &ObjectIdSource,
    ) -> Result<FilamentSet, String> {
        if self.empty {
            return Ok(FilamentSet::new(
                self.level,
                ObjectIds::empty(),
                Mesh::new(Vec::new(), Vec::new()),
                Vec::new(),
                IntensityTable::empty(self.channels.count()),
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
        let missing = self.channels.no_data(self.channels.detection());
        let field = LocalField::new(
            [0, 1, 2].map(|axis| self.extent[axis] as usize),
            [0, 0, 0],
            [0, 0, 0],
            self.intensity,
            self.budget,
            |index| match missing.is_some_and(|no_data| no_data.holds(detection, index)) {
                true => f32::NAN,
                false => detection.value(index),
            },
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
        self.tube.copy_from_slice(&self.mask);
        volume::distance(&self.mask, &self.grid, &mut self.distance);
        if skeleton::rejects_sheet(&self.mask, &self.grid, &self.distance) {
            return Err(
                "the filament mask holds a sheet, and a sheet carries no thinned line to trace"
                    .to_string(),
            );
        }
        skeleton::thin(&mut self.mask, &self.distance, &self.grid);
        let traced = graph::trace(
            &self.mask,
            &self.distance,
            &self.grid,
            self.params.min_length(),
        );
        self.label(&traced);
        let mut accumulators = vec![SampleAccumulator::new(); traced.len() * self.channels.count()];
        for channel in 0..self.channels.count() {
            let measured = if channel == self.channels.detection() {
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
            for (cell, owner) in self.owner.iter().enumerate() {
                if *owner == 0 || self.tube[cell] == 0 {
                    continue;
                }
                let slot = (*owner as usize - 1) * self.channels.count() + channel;
                self.measure(data, channel, cell, &mut accumulators[slot]);
            }
        }
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        for (index, filament) in traced.iter().enumerate() {
            let object = index as u32;
            let Some((mut shaped, mut faces)) = graph::mesh(filament, object) else {
                return Err(format!(
                    "the filament {index} outruns the cells an implicit body may hold"
                ));
            };
            vertices.append(&mut shaped);
            indices.append(&mut faces);
        }
        let objects = traced
            .into_iter()
            .map(|filament| filament.statistics)
            .collect::<Vec<_>>();
        let intensities = IntensityTable::new(
            self.channels.count(),
            accumulators
                .into_iter()
                .map(SampleAccumulator::statistics)
                .collect(),
        );
        FilamentSet::paged(
            self.level,
            ids.allocate(objects.len()),
            store,
            Mesh::new(vertices, indices),
            objects,
            intensities,
        )
        .map_err(|error| error.to_string())
    }

    fn label(&mut self, traced: &[graph::Filament]) {
        for (cell, value) in self.tube.iter().enumerate() {
            if *value != 0 {
                self.owner[cell] = 0;
                self.reach[cell] = u32::MAX;
            }
        }
        for (slot, filament) in traced.iter().enumerate() {
            for node in &filament.nodes {
                self.owner[*node] = slot as u32 + 1;
                self.reach[*node] = 0;
            }
        }
        volume::nearest(&self.tube, &self.grid, &mut self.owner, &mut self.reach);
    }

    fn measure(
        &self,
        data: &VoxelData,
        channel: usize,
        cell: usize,
        accumulator: &mut SampleAccumulator,
    ) {
        let missing = self.channels.no_data(channel);
        let voxel = self.grid.voxel(cell);
        let extent = self.extent;
        let step = self.grid.step;
        let width = extent[0] as usize;
        let height = extent[1] as usize;
        let low = [0, 1, 2].map(|axis| voxel[axis] as u32 * step[axis]);
        let high = [0, 1, 2].map(|axis| ((voxel[axis] as u32 + 1) * step[axis]).min(extent[axis]));
        for z in low[2]..high[2] {
            for y in low[1]..high[1] {
                let base = width * (z as usize * height + y as usize);
                for x in low[0]..high[0] {
                    let index = base + x as usize;
                    if missing.is_some_and(|no_data| no_data.holds(data, index)) {
                        continue;
                    }
                    accumulator.push(data.value(index));
                }
            }
        }
    }
}
