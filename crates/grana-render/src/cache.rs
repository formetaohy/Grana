use grana_volume::{BRICK_SIDE, BrickData, BrickGrid, PADDED_SIDE, Pyramid};
use wgpu::{BindGroupLayout, Device, Queue, Sampler, TextureView};

use crate::MAX_CHANNEL;
use crate::atlas::Atlas;
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
        frame: u64,
        request: BrickRequest,
        data: &BrickData,
    ) -> bool {
        let BrickRequest { level, coord } = request;
        if level >= self.pyramid.level_count() || data.channels().len() != self.channels {
            return false;
        }
        let extent = self.pyramid.level(level).extent();
        if coord.x() >= extent.x().div_ceil(BRICK_SIDE)
            || coord.y() >= extent.y().div_ceil(BRICK_SIDE)
            || coord.z() >= extent.z().div_ceil(BRICK_SIDE)
        {
            return false;
        }
        if self.levels[level].is_none() {
            self.levels[level] = Some(LevelCache::new(
                device,
                resources,
                self.pyramid.level(level),
            ));
        }
        let index = self.levels[level]
            .as_ref()
            .expect("the level cache has just been created")
            .grid
            .index(coord);
        if self.levels[level]
            .as_ref()
            .expect("the level cache exists")
            .slot(index)
            != 0
        {
            return true;
        }
        let Some((slot, evicted)) = self.atlas.allocate(frame) else {
            return false;
        };
        if let Some((old_level, old_index)) = evicted
            && let Some(old_cache) = self.levels[old_level].as_mut()
        {
            old_cache.set_record(queue, old_index, BrickRecord::new(0, [[0.0; 4]; 2]));
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
        self.levels[level]
            .as_mut()
            .expect("the level cache exists")
            .set_record(queue, index, BrickRecord::new(slot + 1, maxima));
        self.atlas.assign(slot, level, index, frame);
        true
    }
}
