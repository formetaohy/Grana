use crate::{
    BrickCoord, BrickData, BrickGrid, ChannelInfo, Extent, Intensity, Level, Pyramid, Spacing,
    VoxelData, VoxelFormat, brick::pad_brick,
};

#[derive(Clone, Debug, PartialEq)]
pub struct Channel {
    name: String,
    tint: [f32; 3],
    intensity: Intensity,
    data: VoxelData,
}

impl Channel {
    pub fn new(name: impl Into<String>, tint: [f32; 3], data: VoxelData) -> Self {
        assert!(
            tint.iter().all(|component| (0.0..=1.0).contains(component)),
            "a channel tint is an srgb triple"
        );
        let intensity = data.range();
        Self {
            name: name.into(),
            tint,
            intensity,
            data,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn tint(&self) -> [f32; 3] {
        self.tint
    }

    pub fn intensity(&self) -> Intensity {
        self.intensity
    }

    pub fn data(&self) -> &VoxelData {
        &self.data
    }

    pub fn format(&self) -> VoxelFormat {
        self.data.format()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Volume {
    extent: Extent,
    spacing: Spacing,
    channels: Vec<Channel>,
}

impl Volume {
    pub fn new(extent: Extent, spacing: Spacing, channels: Vec<Channel>) -> Self {
        assert!(!channels.is_empty(), "a volume needs at least one channel");
        assert!(
            channels
                .iter()
                .all(|channel| channel.data().voxel_count() as u64 == extent.voxel_count()),
            "every channel matches the volume extent"
        );
        Self {
            extent,
            spacing,
            channels,
        }
    }

    pub fn extent(&self) -> Extent {
        self.extent
    }

    pub fn spacing(&self) -> Spacing {
        self.spacing
    }

    pub fn channels(&self) -> &[Channel] {
        &self.channels
    }

    pub fn channel_count(&self) -> usize {
        self.channels.len()
    }

    pub fn world_extent(&self) -> [f32; 3] {
        self.spacing.world_extent(self.extent)
    }

    pub fn pyramid(&self) -> Pyramid {
        let level = Level::new(self.extent, self.spacing);
        let channels = self
            .channels
            .iter()
            .map(|channel| {
                let intensity = channel.intensity();
                let window = if intensity.span() > 0.0 {
                    intensity
                } else {
                    Intensity::new(0.0, 1.0)
                };
                ChannelInfo::new(
                    channel.name(),
                    channel.tint(),
                    intensity,
                    window,
                    channel.format(),
                )
            })
            .collect();
        Pyramid::new(vec![level], channels, 1, 1.0)
    }

    pub fn brick(&self, grid: &BrickGrid, coord: BrickCoord) -> BrickData {
        assert_eq!(
            grid.extent(),
            self.extent,
            "the brick grid matches the volume extent"
        );
        let channels = (0..self.channels.len())
            .map(|channel| self.channel_brick(grid, coord, channel))
            .collect();
        BrickData::new(channels)
    }

    pub fn channel_brick(&self, grid: &BrickGrid, coord: BrickCoord, channel: usize) -> VoxelData {
        assert_eq!(
            grid.extent(),
            self.extent,
            "the brick grid matches the volume extent"
        );
        assert!(
            channel < self.channels.len(),
            "the brick channel lies inside the volume"
        );
        pad_brick(
            self.channels[channel].data(),
            [0, 0, 0],
            self.extent,
            grid.origin(coord),
            self.extent,
        )
    }
}
