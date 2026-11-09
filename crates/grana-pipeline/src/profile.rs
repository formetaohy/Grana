use std::ops::Range;

use grana_profile::{Line, Profile, Source};
use grana_volume::{Extent, VoxelData};

use crate::channels::Channels;

pub struct ProfileSource<'a> {
    channels: &'a Channels,
    time: u32,
}

impl<'a> ProfileSource<'a> {
    pub fn new(channels: &'a Channels, time: u32) -> Self {
        assert!(
            time < channels.pyramid().frame_count(),
            "a profile samples a frame the dataset holds"
        );
        Self { channels, time }
    }

    pub fn profile(&self, line: Line, spacing: f32, names: &[String]) -> Result<Profile, String> {
        grana_profile::sample(self, line, spacing, names)
    }
}

impl Source for ProfileSource<'_> {
    fn extent(&self) -> Extent {
        self.channels.pyramid().level(0).extent()
    }

    fn voxel(&self) -> [f32; 3] {
        let spacing = self.channels.pyramid().level(0).spacing();
        [spacing.x(), spacing.y(), spacing.z()]
    }

    fn read(&self, channel: usize, ranges: &[Range<u32>; 3]) -> Result<VoxelData, String> {
        self.channels
            .read_region(self.time, 0, channel, ranges, [0.0; 3])
            .map_err(|error| error.to_string())
    }
}
