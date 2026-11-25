use crate::Result;
use crate::error::Error;

pub const SIDE: usize = 1024;
pub const FEATURE_SIDE: usize = 64;

const SPLITS: [usize; 5] = [1, 2, 4, 8, 16];
const SPAN_FLOOR: usize = 64;
const DIMENSION: usize = 96;
const HEADS: usize = 8;
const TOKENS: usize = (SIDE / 4) * (SIDE / 4);
const DEEP_TOKENS: usize = FEATURE_SIDE * FEATURE_SIDE;
const NECK_CHANNELS: usize = 256;
const QUERIES: usize = 256;
const STAGING: u64 = 6;

#[derive(Clone, Copy, Debug)]
pub struct Plan {
    splits: usize,
    peak: u64,
}

impl Plan {
    pub fn bytes(self) -> u64 {
        self.peak
    }

    pub(crate) fn splits(self) -> usize {
        self.splits
    }

    pub(crate) fn queries(self) -> usize {
        QUERIES
    }

    pub(crate) fn of(weights: u64, depth: usize, spare: u64) -> Result<Self> {
        let mut least = 0;
        for splits in SPLITS {
            least = peak(weights, depth, splits);
            if least <= spare {
                return Ok(Self {
                    splits,
                    peak: least,
                });
            }
        }
        Err(Error::input(format!(
            "the segmentation model needs {:.1} GiB of graphics memory for a {depth} slice block, above the {:.1} GiB the renderer leaves free",
            least as f64 / (1u64 << 30) as f64,
            spare as f64 / (1u64 << 30) as f64,
        )))
    }
}

fn peak(weights: u64, depth: usize, splits: usize) -> u64 {
    weights + encoder_peak(depth) + decode_peak(depth, splits)
}

fn decode_peak(depth: usize, splits: usize) -> u64 {
    let held = volume(64, 512, depth) + 2 * volume(32, 512, depth) + volume(32, SIDE, depth);
    let span = volume(32, SIDE, depth) / spans(splits) as u64;
    held + (STAGING * span).max(volume(5, SIDE, depth))
}

fn encoder_peak(depth: usize) -> u64 {
    let input = plane(3, SIDE) + plane(1, SIDE);
    let scores = 2 * HEADS as u64 * QUERIES as u64 * DEEP_TOKENS as u64 * 4;
    let blocks = 8 * TOKENS as u64 * DIMENSION as u64 * 4;
    let features = volume(NECK_CHANNELS, FEATURE_SIDE, depth);
    input + scores + blocks + features
}

fn spans(splits: usize) -> usize {
    splits.clamp(1, SIDE / SPAN_FLOOR)
}

fn plane(channels: usize, side: usize) -> u64 {
    channels as u64 * side as u64 * side as u64 * 4
}

fn volume(channels: usize, side: usize, depth: usize) -> u64 {
    plane(channels, side) * depth as u64
}
