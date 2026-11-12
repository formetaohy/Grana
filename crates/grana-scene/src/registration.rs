use std::sync::Arc;

use grana_warp::Warp;

const LIMIT: f32 = 1_048_576.0;
const DIGEST: u64 = 0xcbf2_9ce4_8422_2325;
const DIGEST_PRIME: u64 = 0x0000_0100_0000_01b3;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Registration {
    reference: usize,
    warps: Arc<[Warp]>,
}

impl Registration {
    pub const LIMIT: f32 = LIMIT;

    pub fn new(reference: usize, warps: Vec<Warp>) -> Self {
        assert!(
            warps.is_empty() || reference < warps.len(),
            "a registration reference is one of the aligned channels"
        );
        assert!(
            warps.is_empty() || warps[reference].identity(),
            "the reference channel holds still"
        );
        assert!(
            warps
                .iter()
                .all(|warp| warp.magnitude().is_finite() && warp.magnitude() <= LIMIT),
            "a registration warp stays inside the volume"
        );
        Self {
            reference,
            warps: warps.into(),
        }
    }

    pub fn reference(&self) -> usize {
        self.reference
    }

    pub fn warps(&self) -> &[Warp] {
        &self.warps
    }

    pub fn channels(&self) -> usize {
        self.warps.len()
    }

    pub fn is_empty(&self) -> bool {
        self.warps.is_empty()
    }

    pub fn warp(&self, channel: usize) -> &Warp {
        self.warps.get(channel).unwrap_or(&Warp::IDENTITY)
    }

    pub fn aligned(&self) -> bool {
        self.warps.iter().any(|warp| !warp.identity())
    }

    pub fn deformed(&self) -> bool {
        self.warps.iter().any(Warp::deformed)
    }

    pub fn magnitude(&self) -> f32 {
        self.warps.iter().map(Warp::magnitude).fold(0.0, f32::max)
    }

    pub fn deformation(&self) -> f32 {
        self.warps
            .iter()
            .map(|warp| match warp.field_grid() {
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
            })
            .fold(0.0, f32::max)
    }

    pub fn digest(&self) -> u64 {
        let mut digest = mix(DIGEST, self.reference as u64);
        digest = mix(digest, self.warps.len() as u64);
        for warp in self.warps.iter() {
            digest = mix(digest, warp.digest());
        }
        digest
    }
}

fn mix(digest: u64, value: u64) -> u64 {
    (digest ^ value).wrapping_mul(DIGEST_PRIME)
}
