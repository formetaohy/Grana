use std::sync::Mutex;

use crate::Result;
use crate::error::Error;

pub const SIDE: usize = 1024;
pub const FEATURE_SIDE: usize = 64;

const SPLITS: [usize; 5] = [1, 2, 4, 8, 16];
const SPAN_FLOOR: usize = 64;
const DIMENSION: usize = 96;
const HEADS: usize = 8;
const QUERIES: usize = 256;
const DEEP_TOKENS: usize = FEATURE_SIDE * FEATURE_SIDE;
const NECK_CHANNELS: usize = 256;
const DEEP_CHANNELS: usize = 768;
const LADDER: [usize; 4] = [256, 128, 64, 32];
const OUTPUT_CHANNELS: usize = 5;
const TAIL_COPIES: u64 = 4;
const TAIL_SPANS: u64 = 2;
const BLOCK_SPANS: u64 = 3;
const RESERVED_OVER_LIVE: (u64, u64) = (7, 5);
const PAGE_WASTE: (u64, u64) = (1, 2);

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

    pub(crate) fn of(weights: u64, depth: usize, spare: u64, peaks: &Peaks) -> Result<Self> {
        let mut least = 0;
        for splits in SPLITS {
            least = weights + growth(depth, splits, peaks);
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

#[derive(Default)]
pub(crate) struct Peaks {
    held: Mutex<Vec<Peak>>,
}

#[derive(Clone, Copy)]
struct Peak {
    depth: usize,
    splits: usize,
    bytes: u64,
}

impl Peaks {
    fn lookup(&self, depth: usize, splits: usize) -> Option<u64> {
        self.held
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .iter()
            .find(|peak| peak.depth == depth && peak.splits == splits)
            .map(|peak| peak.bytes)
    }

    pub(crate) fn record(&self, depth: usize, splits: usize, bytes: u64) {
        let mut held = self.held.lock().unwrap_or_else(|error| error.into_inner());
        match held
            .iter_mut()
            .find(|peak| peak.depth == depth && peak.splits == splits)
        {
            Some(peak) => peak.bytes = peak.bytes.max(bytes),
            None => held.push(Peak {
                depth,
                splits,
                bytes,
            }),
        }
    }
}

fn growth(depth: usize, splits: usize, peaks: &Peaks) -> u64 {
    let live = encoder() + frame(depth, splits) + page_waste(depth);
    peaks.lookup(depth, splits).map_or_else(
        || live / RESERVED_OVER_LIVE.1 * RESERVED_OVER_LIVE.0,
        |measured| measured.max(live),
    )
}

fn encoder() -> u64 {
    let windowed = 4 * plane(DIMENSION, SIDE / 4);
    let scores = 2 * HEADS as u64 * QUERIES as u64 * DEEP_TOKENS as u64 * 4;
    let deep = 4 * plane(DEEP_CHANNELS, FEATURE_SIDE);
    let neck = 2 * plane(NECK_CHANNELS, FEATURE_SIDE);
    windowed + scores + deep + neck
}

fn frame(depth: usize, splits: usize) -> u64 {
    let held = volume(LADDER[0], FEATURE_SIDE, depth)
        + volume(LADDER[0], FEATURE_SIDE, depth)
        + volume(LADDER[0], SIDE / 8, depth)
        + volume(LADDER[1], SIDE / 4, depth)
        + volume(LADDER[2], SIDE / 2, depth);
    let branch = volume(LADDER[3], SIDE, depth);
    let output = volume(OUTPUT_CHANNELS, SIDE, depth);
    let span = branch / spans(splits) as u64;
    let blocks = held + branch + BLOCK_SPANS * span;
    let tail = held + branch + TAIL_COPIES * output + TAIL_SPANS * span;
    blocks.max(tail)
}

fn page_waste(depth: usize) -> u64 {
    volume(LADDER[3], SIDE, depth) / PAGE_WASTE.1 * PAGE_WASTE.0
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
