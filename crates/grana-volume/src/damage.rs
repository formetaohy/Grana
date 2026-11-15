use crate::{ChannelSet, Region};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Damage {
    region: Region,
    channels: ChannelSet,
}

impl Damage {
    pub fn new(region: Region, channels: ChannelSet) -> Option<Self> {
        assert!(region.enabled(), "a damage names the region that changed");
        (!channels.is_empty()).then_some(Self { region, channels })
    }

    pub fn region(self) -> Region {
        self.region
    }

    pub fn channels(self) -> ChannelSet {
        self.channels
    }
}
