use grana_volume::{Extent, Pyramid};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Series {
    extent: Extent,
    channels: usize,
    frames: u32,
    name: Option<String>,
}

impl Series {
    pub(crate) fn new(extent: Extent, channels: usize, frames: u32, name: Option<String>) -> Self {
        Self {
            extent,
            channels,
            frames,
            name,
        }
    }

    pub fn of(pyramid: &Pyramid) -> Self {
        Self::new(
            pyramid.level(0).extent(),
            pyramid.channel_count(),
            pyramid.frame_count(),
            None,
        )
    }

    pub fn extent(&self) -> Extent {
        self.extent
    }

    pub fn channels(&self) -> usize {
        self.channels
    }

    pub fn frames(&self) -> u32 {
        self.frames
    }

    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }
}
