use grana_warp::{Warp, Warps};

use crate::drift::Drift;

const DIGEST: u64 = 0xcbf2_9ce4_8422_2325;
const DIGEST_PRIME: u64 = 0x0000_0100_0000_01b3;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Drifts {
    entries: Vec<Option<Drift>>,
}

impl Drifts {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, channel: usize) -> Option<&Drift> {
        self.entries.get(channel).and_then(Option::as_ref)
    }

    pub fn held(&self) -> impl Iterator<Item = &Drift> {
        self.entries.iter().flatten()
    }

    pub fn len(&self) -> usize {
        self.held().count()
    }

    pub fn is_empty(&self) -> bool {
        self.held().next().is_none()
    }

    pub fn channels(&self) -> usize {
        self.entries.len()
    }

    pub fn warp(&self, channel: usize, time: u32) -> &Warp {
        self.get(channel)
            .map_or(&Warp::IDENTITY, |drift| drift.warp(time))
    }

    pub fn warps(&self, time: u32, channels: usize) -> Warps {
        Warps::new(
            (0..channels)
                .map(|channel| self.warp(channel, time).clone())
                .collect(),
        )
    }

    pub fn placed(&self, drift: Drift) -> Self {
        let channel = drift.channel();
        assert!(
            channel < grana_volume::CHANNEL_LIMIT,
            "a drift correction aligns a bounded channel"
        );
        let mut entries = self.entries.clone();
        if entries.len() <= channel {
            entries.resize(channel + 1, None);
        }
        entries[channel] = Some(drift);
        Self { entries }
    }

    pub fn without(&self, channel: usize) -> Self {
        let mut entries = self.entries.clone();
        if let Some(slot) = entries.get_mut(channel) {
            *slot = None;
        }
        Self { entries }
    }

    pub fn fitted(&self, channels: usize) -> Self {
        let mut entries = self.entries.clone();
        entries.truncate(channels);
        while entries.last().is_some_and(Option::is_none) {
            entries.pop();
        }
        Self { entries }
    }

    pub fn digest(&self) -> u64 {
        let mut digest = mix(DIGEST, self.entries.len() as u64);
        for (channel, entry) in self.entries.iter().enumerate() {
            digest = mix(digest, channel as u64);
            match entry {
                Some(drift) => digest = mix(digest, drift.digest()),
                None => digest = mix(digest, 0),
            }
        }
        digest
    }
}

fn mix(digest: u64, value: u64) -> u64 {
    (digest ^ value).wrapping_mul(DIGEST_PRIME)
}
