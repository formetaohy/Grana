use glam::Vec3;
use grana_volume::{Extent, Intensity, Spacing, Volume, VoxelFormat};

pub struct Dataset {
    pub name: String,
    pub extent: Extent,
    pub spacing: Spacing,
    pub format: VoxelFormat,
    pub intensity: Intensity,
    pub level_count: u32,
    pub world_size: Vec3,
}

impl Dataset {
    pub fn new(name: impl Into<String>, volume: &Volume) -> Self {
        Self {
            name: name.into(),
            extent: volume.extent(),
            spacing: volume.spacing(),
            format: volume.format(),
            intensity: volume.intensity(),
            level_count: volume.level_count(),
            world_size: Vec3::from(volume.world_extent()),
        }
    }
}
