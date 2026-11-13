use grana_warp::Warp;

use crate::drift::Drift;
use crate::registration::Registration;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LocalizationAlignment {
    channel: usize,
    drift: Drift,
    registration: Registration,
}

impl LocalizationAlignment {
    pub fn new(channel: usize, drift: Drift, registration: Registration) -> Self {
        assert!(
            drift.is_empty() || drift.channel() == channel,
            "a localization aligns with the drift of the channel it was measured in"
        );
        Self {
            channel,
            drift,
            registration,
        }
    }

    pub fn channel(&self) -> usize {
        self.channel
    }

    pub fn drift(&self) -> &Drift {
        &self.drift
    }

    pub fn registration(&self) -> &Registration {
        &self.registration
    }

    pub fn warp(&self, time: u32) -> Warp {
        self.drift
            .warp(time)
            .added(self.registration.warp(self.channel))
    }

    pub fn is_empty(&self) -> bool {
        self.drift.is_empty() && self.registration.is_empty()
    }
}
