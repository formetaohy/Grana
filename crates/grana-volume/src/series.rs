use std::ops::Range;

use crate::{
    BrickCoord, BrickData, BrickGrid, Calibration, ChannelInfo, Histogram, Intensity, Level,
    Pyramid, Volume, VoxelData,
};

#[derive(Clone, Debug, PartialEq)]
pub struct VolumeSeries {
    frames: Vec<Volume>,
    pyramid: Pyramid,
}

impl VolumeSeries {
    pub fn new(frames: Vec<Volume>, frame_interval: f32) -> Self {
        assert!(
            !frames.is_empty(),
            "a VolumeSeries holds at least one frame"
        );
        assert!(
            u32::try_from(frames.len()).is_ok(),
            "the VolumeSeries holds more frames than the time axis can index"
        );
        let first = &frames[0];
        for frame in &frames[1..] {
            assert_eq!(
                frame.extent(),
                first.extent(),
                "every VolumeSeries frame covers the same extent"
            );
            assert_eq!(
                frame.spacing(),
                first.spacing(),
                "every VolumeSeries frame has the same spacing"
            );
            assert_eq!(
                frame.channels().len(),
                first.channels().len(),
                "every VolumeSeries frame carries the same channels"
            );
            for (frame_channel, first_channel) in frame.channels().iter().zip(first.channels()) {
                assert_eq!(
                    frame_channel.name(),
                    first_channel.name(),
                    "VolumeSeries channels keep their names"
                );
                assert_eq!(
                    frame_channel.tint(),
                    first_channel.tint(),
                    "VolumeSeries channels keep their tints"
                );
                assert_eq!(
                    frame_channel.format(),
                    first_channel.format(),
                    "VolumeSeries channels keep their voxel format"
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
            .expect("the frame lies inside the VolumeSeries")
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
                let channel = &first.channels()[index];
                let info = match channel.color_table() {
                    Some(color_table) => ChannelInfo::declared(
                        channel.name(),
                        channel.tint(),
                        color_table.indices(),
                        channel.format(),
                    )
                    .with_color_table(color_table.clone())
                    .with_background(channel.background()),
                    None => {
                        let statistics = frames.iter().fold(channel.statistics(), |held, frame| {
                            held.folded(frame.channels()[index].statistics())
                        });
                        let info = ChannelInfo::new(
                            channel.name(),
                            channel.tint(),
                            statistics,
                            channel.format(),
                        );
                        match profile(frames, index, statistics.intensity()) {
                            Some(histogram) => info.with_histogram(histogram),
                            None => info,
                        }
                    }
                };
                match channel.no_data() {
                    Some(no_data) => info.with_no_data(no_data),
                    None => info,
                }
            })
            .collect();
        Pyramid::new(
            vec![Level::new(first.extent(), first.spacing())],
            1,
            channels,
            frames.len() as u32,
            frame_interval,
            Calibration::Recorded,
        )
    }
}

fn profile(frames: &[Volume], index: usize, intensity: Intensity) -> Option<Histogram> {
    let mut folded: Option<Histogram> = None;
    for frame in frames {
        let Some(held) = frame.channels()[index].histogram() else {
            continue;
        };
        let rebased = held.rebased(intensity);
        match folded.as_mut() {
            Some(folded) => folded.merge(&rebased),
            None => folded = Some(rebased),
        }
    }
    folded
}
