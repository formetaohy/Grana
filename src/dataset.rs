use glam::Vec3;
use grana_volume::{ChannelInfo, Extent, Pyramid, Spacing, VoxelFormat};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    Synthetic,
    Zarr,
}

#[derive(Clone, Debug)]
pub struct Dataset {
    name: String,
    pyramid: Pyramid,
    origin: Origin,
}

impl Dataset {
    pub fn synthetic(name: impl Into<String>, pyramid: Pyramid) -> Self {
        Self {
            name: name.into(),
            pyramid,
            origin: Origin::Synthetic,
        }
    }

    pub fn zarr(name: impl Into<String>, pyramid: Pyramid) -> Self {
        Self {
            name: name.into(),
            pyramid,
            origin: Origin::Zarr,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn pyramid(&self) -> &Pyramid {
        &self.pyramid
    }

    pub fn origin(&self) -> Origin {
        self.origin
    }

    pub fn extent(&self) -> Extent {
        self.pyramid.level(0).extent()
    }

    pub fn spacing(&self) -> Spacing {
        self.pyramid.level(0).spacing()
    }

    pub fn world_size(&self) -> Vec3 {
        Vec3::from(self.pyramid.level(0).world_extent())
    }

    pub fn voxel_size(&self) -> Vec3 {
        let spacing = self.spacing();
        Vec3::new(spacing.x(), spacing.y(), spacing.z())
    }

    pub fn level_count(&self) -> usize {
        self.pyramid.level_count()
    }

    pub fn frame_count(&self) -> u32 {
        self.pyramid.frame_count()
    }

    pub fn frame_interval(&self) -> f32 {
        self.pyramid.frame_interval()
    }

    pub fn channels(&self) -> &[ChannelInfo] {
        self.pyramid.channels()
    }

    pub fn format(&self) -> Option<VoxelFormat> {
        let first = self.pyramid.channels().first()?.format();
        self.pyramid
            .channels()
            .iter()
            .all(|channel| channel.format() == first)
            .then_some(first)
    }
}
