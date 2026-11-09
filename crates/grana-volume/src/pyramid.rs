use crate::{ChannelInfo, Level, Region};

#[derive(Clone, Debug, PartialEq)]
pub struct Pyramid {
    levels: Vec<Level>,
    stored: usize,
    channels: Vec<ChannelInfo>,
    frames: u32,
    frame_interval: f32,
    placement: [f32; 3],
}

impl Pyramid {
    pub fn new(
        levels: Vec<Level>,
        stored: usize,
        channels: Vec<ChannelInfo>,
        frames: u32,
        frame_interval: f32,
    ) -> Self {
        Self::placed(levels, stored, channels, frames, frame_interval, [0.0; 3])
    }

    pub fn placed(
        levels: Vec<Level>,
        stored: usize,
        channels: Vec<ChannelInfo>,
        frames: u32,
        frame_interval: f32,
        placement: [f32; 3],
    ) -> Self {
        assert!(!levels.is_empty(), "a pyramid needs at least one level");
        assert!(
            stored > 0 && stored <= levels.len(),
            "the stored levels form a prefix of the pyramid"
        );
        assert!(!channels.is_empty(), "a pyramid needs at least one channel");
        assert!(frames > 0, "a pyramid holds at least one frame");
        assert!(
            frame_interval.is_finite() && frame_interval > 0.0,
            "the frame interval is a positive duration"
        );
        assert!(
            placement.iter().all(|value| value.is_finite()),
            "a pyramid placement is finite"
        );
        for pair in levels.windows(2) {
            let (fine, coarse) = (pair[0], pair[1]);
            let fine_extent = fine.extent();
            let coarse_extent = coarse.extent();
            assert!(
                fine_extent.x() >= coarse_extent.x()
                    && fine_extent.y() >= coarse_extent.y()
                    && fine_extent.z() >= coarse_extent.z(),
                "pyramid levels must shrink with every step"
            );
            let fine_spacing = fine.spacing();
            let coarse_spacing = coarse.spacing();
            assert!(
                fine_spacing.x() <= coarse_spacing.x()
                    && fine_spacing.y() <= coarse_spacing.y()
                    && fine_spacing.z() <= coarse_spacing.z(),
                "pyramid voxels must grow with every step"
            );
        }
        Self {
            levels,
            stored,
            channels,
            frames,
            frame_interval,
            placement,
        }
    }

    pub fn levels(&self) -> &[Level] {
        &self.levels
    }

    pub fn level(&self, index: usize) -> Level {
        self.levels[index]
    }

    pub fn level_count(&self) -> usize {
        self.levels.len()
    }

    pub fn stored(&self) -> usize {
        self.stored
    }

    pub fn placement(&self) -> [f32; 3] {
        self.placement
    }

    pub fn channels(&self) -> &[ChannelInfo] {
        &self.channels
    }

    pub fn channel_count(&self) -> usize {
        self.channels.len()
    }

    pub fn with_channels(&self, channels: Vec<ChannelInfo>) -> Pyramid {
        assert!(!channels.is_empty(), "a pyramid needs at least one channel");
        Pyramid {
            levels: self.levels.clone(),
            stored: self.stored,
            channels,
            frames: self.frames,
            frame_interval: self.frame_interval,
            placement: self.placement,
        }
    }

    pub fn frame_count(&self) -> u32 {
        self.frames
    }

    pub fn brick_bytes(&self) -> u64 {
        let voxels = u64::from(crate::PADDED_SIDE).pow(3);
        let channel: u64 = self
            .channels
            .iter()
            .map(|info| match info.format() {
                crate::VoxelFormat::U8 => 1,
                crate::VoxelFormat::U16 => 2,
                crate::VoxelFormat::F32 => 4,
            })
            .sum();
        voxels * channel
    }

    pub fn frame_interval(&self) -> f32 {
        self.frame_interval
    }

    pub fn fit_level(&self, region: Region, voxels: u64) -> Option<usize> {
        assert!(voxels > 0, "a level budget covers at least one voxel");
        (0..self.levels.len()).find(|index| {
            region
                .voxel_range(self.levels[*index])
                .is_none_or(|ranges| {
                    ranges
                        .iter()
                        .map(|range| u64::from(range.end - range.start))
                        .product::<u64>()
                        <= voxels
                })
        })
    }
}
