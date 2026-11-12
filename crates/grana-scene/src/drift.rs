use std::sync::Arc;

use grana_warp::Warp;

const LIMIT: f32 = 1_048_576.0;
const DIGEST: u64 = 0xcbf2_9ce4_8422_2325;
const DIGEST_PRIME: u64 = 0x0000_0100_0000_01b3;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Drift {
    channel: usize,
    reference: u32,
    warps: Arc<[Warp]>,
}

impl Drift {
    pub const LIMIT: f32 = LIMIT;

    pub fn new(channel: usize, reference: u32, warps: Vec<Warp>) -> Self {
        assert!(
            warps
                .iter()
                .all(|warp| warp.magnitude().is_finite() && warp.magnitude() <= LIMIT),
            "a drift warp stays inside the volume"
        );
        assert!(
            warps.is_empty() || (reference as usize) < warps.len(),
            "a drift reference is one of the aligned frames"
        );
        assert!(
            warps.is_empty() || warps[reference as usize].identity(),
            "the reference frame holds still"
        );
        Self {
            channel,
            reference,
            warps: warps.into(),
        }
    }

    pub fn channel(&self) -> usize {
        self.channel
    }

    pub fn reference(&self) -> u32 {
        self.reference
    }

    pub fn warps(&self) -> &[Warp] {
        &self.warps
    }

    pub fn is_empty(&self) -> bool {
        self.warps.is_empty()
    }

    pub fn frames(&self) -> u32 {
        self.warps.len() as u32
    }

    pub fn warp(&self, time: u32) -> &Warp {
        self.warps.get(time as usize).unwrap_or(&Warp::IDENTITY)
    }

    pub fn translation(&self, time: u32) -> [f32; 3] {
        self.warp(time).translation()
    }

    pub fn magnitude(&self) -> f32 {
        self.warps.iter().map(Warp::magnitude).fold(0.0, f32::max)
    }

    pub fn deformed(&self) -> bool {
        self.warps.iter().any(Warp::deformed)
    }

    pub fn deformation(&self, time: u32) -> f32 {
        let warp = self.warp(time);
        match warp.field_grid() {
            Some(grid) => {
                let translation = warp.translation();
                grid.values()
                    .iter()
                    .map(|value| {
                        let delta = [0, 1, 2].map(|axis| value[axis] - translation[axis]);
                        delta.iter().map(|value| value * value).sum::<f32>().sqrt()
                    })
                    .fold(0.0, f32::max)
            }
            None => 0.0,
        }
    }

    pub fn referenced_at(&self, reference: u32) -> Self {
        assert!(
            (reference as usize) < self.warps.len(),
            "a drift reference is one of the aligned frames"
        );
        let anchor = &self.warps[reference as usize];
        let warps = self
            .warps
            .iter()
            .map(|warp| warp.relative_to(anchor))
            .collect();
        Self::new(self.channel, reference, warps)
    }

    pub fn translated(&self, time: u32, delta: [f32; 3]) -> Self {
        assert!(
            (time as usize) < self.warps.len(),
            "a drift warp belongs to an aligned frame"
        );
        assert!(time != self.reference, "the reference frame holds still");
        let mut warps = self.warps.to_vec();
        warps[time as usize] = warps[time as usize].translated(delta);
        Self::new(self.channel, self.reference, warps)
    }

    pub fn placed(&self, time: u32, translation: [f32; 3]) -> Self {
        assert!(
            (time as usize) < self.warps.len(),
            "a drift warp belongs to an aligned frame"
        );
        assert!(time != self.reference, "the reference frame holds still");
        let current = self.warp(time).translation();
        let delta = [0, 1, 2].map(|axis| translation[axis] - current[axis]);
        self.translated(time, delta)
    }

    pub fn digest(&self) -> u64 {
        let mut digest = mix(DIGEST, self.channel as u64);
        digest = mix(digest, u64::from(self.reference));
        for warp in self.warps.iter() {
            digest = mix(digest, warp.digest());
        }
        digest
    }
}

fn mix(digest: u64, value: u64) -> u64 {
    (digest ^ value).wrapping_mul(DIGEST_PRIME)
}
