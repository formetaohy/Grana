use crate::{Channel, Extent, Spacing};

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
            channels.iter().all(|channel| channel.extent() == extent),
            "every channel shares the volume extent"
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

    pub fn channel_count(&self) -> u32 {
        self.channels.len() as u32
    }

    pub fn level_count(&self) -> u32 {
        self.channels[0].level_count()
    }

    pub fn world_extent(&self) -> [f32; 3] {
        self.spacing.world_extent(self.extent)
    }
}
