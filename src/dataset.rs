use std::path::Path;

use glam::Vec3;
use grana_render::MAX_CHANNEL;
use grana_volume::{Extent, Intensity, Spacing, Volume, VoxelFormat};
use grana_zarr::{Image, LevelInfo};

pub const TEXTURE_BUDGET: u64 = 512 << 20;

#[derive(Clone, Copy, Debug)]
pub struct LoadLimit {
    pub max_dimension: u32,
    pub budget: u64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SourceLevel {
    pub extent: Extent,
    pub level: usize,
    pub level_count: usize,
}

#[derive(Clone, Debug)]
pub struct ChannelSummary {
    pub name: String,
    pub tint: [f32; 3],
    pub intensity: Intensity,
    pub window: Intensity,
    pub format: VoxelFormat,
}

#[derive(Clone, Debug)]
pub struct Dataset {
    pub name: String,
    pub extent: Extent,
    pub spacing: Spacing,
    pub level_count: u32,
    pub source: Option<SourceLevel>,
    pub channels: Vec<ChannelSummary>,
}

impl Dataset {
    pub fn synthetic(name: impl Into<String>, volume: &Volume) -> Self {
        Self {
            name: name.into(),
            extent: volume.extent(),
            spacing: volume.spacing(),
            level_count: volume.level_count(),
            source: None,
            channels: summarize(volume),
        }
    }

    pub fn open(path: &Path, limit: LoadLimit) -> Result<(Self, Volume), String> {
        let image = Image::open(path).map_err(|error| error.to_string())?;
        let channel_count = image.channels().len();
        if channel_count > MAX_CHANNEL {
            return Err(format!(
                "the dataset has {channel_count} channels, grana displays at most {MAX_CHANNEL}"
            ));
        }
        let levels = image.levels().collect::<Vec<_>>();
        let level = select_level(&levels, channel_count, limit)?;
        let volume = image.read(level).map_err(|error| error.to_string())?;
        let mut dataset = Self::synthetic(image.name(), &volume);
        dataset.source = Some(SourceLevel {
            extent: levels[0].extent,
            level,
            level_count: levels.len(),
        });
        Ok((dataset, volume))
    }

    pub fn world_size(&self) -> Vec3 {
        Vec3::from(self.spacing.world_extent(self.extent))
    }

    pub fn format(&self) -> Option<VoxelFormat> {
        let first = self.channels.first()?.format;
        self.channels
            .iter()
            .all(|channel| channel.format == first)
            .then_some(first)
    }
}

fn summarize(volume: &Volume) -> Vec<ChannelSummary> {
    volume
        .channels()
        .iter()
        .map(|channel| ChannelSummary {
            name: channel.name().to_string(),
            tint: channel.tint(),
            intensity: channel.intensity(),
            window: channel.window(),
            format: channel.format(),
        })
        .collect()
}

fn select_level(
    levels: &[LevelInfo],
    channel_count: usize,
    limit: LoadLimit,
) -> Result<usize, String> {
    levels
        .iter()
        .enumerate()
        .find(|(_, level)| {
            let extent = level.extent;
            extent.x() <= limit.max_dimension
                && extent.y() <= limit.max_dimension
                && extent.z() <= limit.max_dimension
                && pyramid_voxels(extent) * channel_count as u64 * size_of::<u16>() as u64
                    <= limit.budget
        })
        .map(|(index, _)| index)
        .ok_or_else(|| {
            format!(
                "no resolution level of the {channel_count}-channel dataset fits the {} MiB texture budget",
                limit.budget >> 20
            )
        })
}

fn pyramid_voxels(extent: Extent) -> u64 {
    let mut voxels = 0;
    let mut level = extent;
    loop {
        voxels += level.voxel_count();
        if level.max_dimension() <= 1 {
            return voxels;
        }
        level = level.halve();
    }
}
