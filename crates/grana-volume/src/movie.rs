use std::ops::Range;

use crate::{
    BrickCoord, BrickData, BrickGrid, ChannelInfo, Intensity, Level, Pyramid, Volume, VoxelData,
};

#[derive(Clone, Debug, PartialEq)]
pub struct Movie {
    frames: Vec<Volume>,
    pyramid: Pyramid,
}

impl Movie {
    pub fn new(frames: Vec<Volume>, frame_interval: f32) -> Self {
        assert!(!frames.is_empty(), "a movie holds at least one frame");
        assert!(
            u32::try_from(frames.len()).is_ok(),
            "the movie holds more frames than the time axis can index"
        );
        let first = &frames[0];
        for frame in &frames[1..] {
            assert_eq!(
                frame.extent(),
                first.extent(),
                "every movie frame covers the same extent"
            );
            assert_eq!(
                frame.spacing(),
                first.spacing(),
                "every movie frame has the same spacing"
            );
            assert_eq!(
                frame.channels().len(),
                first.channels().len(),
                "every movie frame carries the same channels"
            );
            for (frame_channel, first_channel) in frame.channels().iter().zip(first.channels()) {
                assert_eq!(
                    frame_channel.name(),
                    first_channel.name(),
                    "movie channels keep their names"
                );
                assert_eq!(
                    frame_channel.tint(),
                    first_channel.tint(),
                    "movie channels keep their tints"
                );
                assert_eq!(
                    frame_channel.format(),
                    first_channel.format(),
                    "movie channels keep their voxel format"
                );
            }
        }
        let pyramid = Self::pyramid_of(&frames, frame_interval);
        Self { frames, pyramid }
    }

    pub fn pyramid(&self) -> &Pyramid {
        &self.pyramid
    }

    pub fn frame_count(&self) -> u32 {
        self.frames.len() as u32
    }

    pub fn frame(&self, time: u32) -> &Volume {
        self.frames
            .get(time as usize)
            .expect("the frame lies inside the movie")
    }

    pub fn brick(&self, time: u32, grid: &BrickGrid, coord: BrickCoord) -> BrickData {
        let volume = self.frame(time);
        let channels = (0..volume.channel_count())
            .map(|channel| volume.channel_brick(grid, coord, channel))
            .collect();
        BrickData::new(channels)
    }

    pub fn channel_brick(
        &self,
        time: u32,
        grid: &BrickGrid,
        coord: BrickCoord,
        channel: usize,
    ) -> VoxelData {
        self.frame(time).channel_brick(grid, coord, channel)
    }

    pub fn region(
        &self,
        time: u32,
        channel: usize,
        x: Range<u32>,
        y: Range<u32>,
        z: Range<u32>,
    ) -> VoxelData {
        let volume = self.frame(time);
        volume.channels()[channel]
            .data()
            .region(volume.extent(), x, y, z)
    }

    fn pyramid_of(frames: &[Volume], frame_interval: f32) -> Pyramid {
        let first = &frames[0];
        let channels = (0..first.channel_count())
            .map(|index| {
                let mut low = f32::INFINITY;
                let mut high = f32::NEG_INFINITY;
                for frame in frames {
                    let intensity = frame.channels()[index].intensity();
                    low = low.min(intensity.min());
                    high = high.max(intensity.max());
                }
                let intensity = Intensity::new(low, high);
                let window = if intensity.span() > 0.0 {
                    intensity
                } else {
                    Intensity::new(0.0, 1.0)
                };
                let channel = &first.channels()[index];
                ChannelInfo::new(
                    channel.name(),
                    channel.tint(),
                    intensity,
                    window,
                    channel.format(),
                )
            })
            .collect();
        Pyramid::new(
            vec![Level::new(first.extent(), first.spacing())],
            channels,
            frames.len() as u32,
            frame_interval,
        )
    }
}
