use crate::{ChannelInfo, Level};

#[derive(Clone, Debug, PartialEq)]
pub struct Pyramid {
    levels: Vec<Level>,
    channels: Vec<ChannelInfo>,
    frames: u32,
    frame_interval: f32,
}

impl Pyramid {
    pub fn new(
        levels: Vec<Level>,
        channels: Vec<ChannelInfo>,
        frames: u32,
        frame_interval: f32,
    ) -> Self {
        assert!(!levels.is_empty(), "a pyramid needs at least one level");
        assert!(!channels.is_empty(), "a pyramid needs at least one channel");
        assert!(frames > 0, "a pyramid holds at least one frame");
        assert!(
            frame_interval.is_finite() && frame_interval > 0.0,
            "the frame interval is a positive duration"
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
            channels,
            frames,
            frame_interval,
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

    pub fn channels(&self) -> &[ChannelInfo] {
        &self.channels
    }

    pub fn channel_count(&self) -> usize {
        self.channels.len()
    }

    pub fn frame_count(&self) -> u32 {
        self.frames
    }

    pub fn frame_interval(&self) -> f32 {
        self.frame_interval
    }
}
