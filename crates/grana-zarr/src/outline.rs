use grana_volume::Extent;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outline {
    extent: Extent,
    channels: u32,
    frames: u32,
}

impl Outline {
    pub fn new(extent: Extent, channels: u32, frames: u32) -> Self {
        assert!(channels > 0, "an outline covers at least one channel");
        assert!(frames > 0, "an outline covers at least one frame");
        Self {
            extent,
            channels,
            frames,
        }
    }

    pub fn extent(&self) -> Extent {
        self.extent
    }

    pub fn channels(&self) -> u32 {
        self.channels
    }

    pub fn frames(&self) -> u32 {
        self.frames
    }
}
