use grana_volume::{Extent, Pyramid};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Series {
    extent: Extent,
    channels: usize,
    frames: u32,
}

impl Series {
    pub(crate) fn new(extent: Extent, channels: usize, frames: u32) -> Self {
        Self {
            extent,
            channels,
            frames,
        }
    }

    pub fn of(pyramid: &Pyramid) -> Self {
        Self::new(
            pyramid.level(0).extent(),
            pyramid.channel_count(),
            pyramid.frame_count(),
        )
    }

    pub fn extent(self) -> Extent {
        self.extent
    }

    pub fn channels(self) -> usize {
        self.channels
    }

    pub fn frames(self) -> u32 {
        self.frames
    }
}
