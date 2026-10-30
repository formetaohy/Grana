use grana_volume::{BRICK_SIDE, BrickData, BrickGrid, PADDED_SIDE, Pyramid};
use wgpu::{BindGroupLayout, Device, Queue, Sampler, TextureView};

use crate::MAX_CHANNEL;
use crate::atlas::{Atlas, ResidentBrick};
use crate::level::{BrickRecord, LevelCache, Resources};
use crate::renderer::BrickRequest;

pub(crate) struct Cache {
    pub pyramid: Pyramid,
    pub atlas: Atlas,
    pub levels: Vec<Option<LevelCache>>,
    pub volume_level: Option<usize>,
    pub section_level: Option<usize>,
    pub channels: usize,
}

impl Cache {
    pub fn new(
        device: &Device,
        pyramid: &Pyramid,
        atlas_layout: &BindGroupLayout,
        dummy: &TextureView,
        sampler: &Sampler,
        budget: u64,
    ) -> Self {
        let channels = pyramid.channel_count();
        assert!(
            (1..=MAX_CHANNEL).contains(&channels),
            "a volume holds between one and {MAX_CHANNEL} channels"
        );
        assert!(
            !pyramid.levels().is_empty(),
            "a pyramid holds at least one level"
        );
        let finest = BrickGrid::new(pyramid.level(0).extent()).count() as u64;
        let slot_bytes = (PADDED_SIDE as u64).pow(3) * size_of::<u16>() as u64 * channels as u64;
        let requested = (budget / slot_bytes).max(1).min(finest);
        let atlas = Atlas::new(
            device,
            atlas_layout,
            dummy,
            sampler,
            channels,
            requested,
            device.limits().max_texture_dimension_3d,
        );
        Self {
            pyramid: pyramid.clone(),
            atlas,
            levels: (0..pyramid.level_count()).map(|_| None).collect(),
            volume_level: None,
            section_level: None,
            channels,
        }
    }

    pub fn upload(
        &mut self,
        device: &Device,
        queue: &Queue,
        resources: &Resources<'_>,
        tick: u64,
        request: BrickRequest,
        data: &BrickData,
    ) -> bool {
        let BrickRequest { time, level, coord } = request;
        if level >= self.pyramid.level_count()
            || time >= self.pyramid.frame_count()
            || data.channels().len() != self.channels
        {
            return false;
        }
        let extent = self.pyramid.level(level).extent();
        if coord.x() >= extent.x().div_ceil(BRICK_SIDE)
            || coord.y() >= extent.y().div_ceil(BRICK_SIDE)
            || coord.z() >= extent.z().div_ceil(BRICK_SIDE)
        {
            return false;
        }
        let grid = BrickGrid::new(extent);
        let index = grid.index(coord);
        if self.atlas.slot(time, level, index).is_some() {
            return true;
        }
        if self.levels[level].is_none() {
            self.levels[level] = Some(LevelCache::new(
                device,
                resources,
                level,
                self.pyramid.level(level),
            ));
        }
        let Some((slot, evicted)) = self.atlas.allocate(tick) else {
            return false;
        };
        if let Some(brick) = evicted
            && let Some(cache) = self.levels[brick.level].as_mut()
        {
            cache.clear_record(queue, brick.time, brick.index);
        }
        let mut maxima = [[0.0f32; 4]; 2];
        for channel in 0..self.channels {
            let intensity = self.pyramid.channels()[channel].intensity();
            let source = &data.channels()[channel];
            let mut units = vec![0u16; source.voxel_count()];
            source.write_units(intensity, &mut units);
            self.atlas.write(queue, channel, slot, &units);
            maxima[channel / 4][channel % 4] = source.unit_max(intensity);
        }
        let brick = ResidentBrick {
            time,
            level,
            index,
            maxima,
        };
        self.atlas.assign(slot, brick, tick);
        self.levels[level]
            .as_mut()
            .expect("the level cache exists")
            .set_record(queue, time, index, BrickRecord::new(slot + 1, maxima));
        true
    }
}
