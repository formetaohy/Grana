use std::sync::Arc;

use crate::Warp;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Warps {
    entries: Arc<[Warp]>,
}

impl Warps {
    pub fn new(warps: Vec<Warp>) -> Self {
        Self {
            entries: warps.into(),
        }
    }

    pub fn single(warp: Warp) -> Self {
        Self::new(vec![warp])
    }

    pub fn get(&self, channel: usize) -> &Warp {
        self.entries.get(channel).unwrap_or(&Warp::IDENTITY)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn entries(&self) -> &[Warp] {
        &self.entries
    }

    pub fn identity(&self) -> bool {
        self.entries.iter().all(Warp::identity)
    }

    pub fn digest(&self) -> u64 {
        let mut digest = MIX;
        digest = mix(digest, self.entries.len() as u64);
        for warp in self.entries.iter() {
            digest = mix(digest, warp.digest());
        }
        digest
    }
}

const MIX: u64 = 0xcbf2_9ce4_8422_2325;
const PRIME: u64 = 0x0000_0100_0000_01b3;

fn mix(digest: u64, value: u64) -> u64 {
    (digest ^ value).wrapping_mul(PRIME)
}
