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
const OUTPUT_CHANNELS: usize = 4;
use crate::model::decoder::{RETENTION, SHIFTS, SLAB};

const LEVEL_CHANNELS: [usize; 7] = [256, 256, 128, 128, 64, 64, 32];
const LEVEL_SIDES: [usize; 7] = [64, 128, 256, 128, 256, 512, 512];
const RESERVED_OVER_LIVE: (u64, u64) = (7, 5);
const PAGE_WASTE: (u64, u64) = (3, 2);
const CONV_COPIES: u64 = 4;
const DEPTH_KEY_CAP: usize = 16;

#[derive(Clone, Copy, Debug)]
pub struct Plan {
    splits: usize,
    peak: u64,
}

impl Plan {
    pub fn bytes(self) -> u64 {
        self.peak
    }

    pub fn splits(self) -> usize {
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
            "the segmentation model needs {:.1} GiB of graphics memory for a {depth} slice volume, above the {:.1} GiB the renderer leaves free",
            least as f64 / (1u64 << 30) as f64,
            spare as f64 / (1u64 << 30) as f64,
        )))
    }

    #[doc(hidden)]
    pub fn forced(splits: usize, peak: u64) -> Self {
        Self { splits, peak }
    }

    pub(crate) fn estimate(weights: u64, depth: usize, splits: usize, peaks: &Peaks) -> u64 {
        weights + growth(depth, splits, peaks)
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
        let depth = depth.min(DEPTH_KEY_CAP);
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
    let live = encoder() + streaming(depth, splits);
    match peaks.lookup(depth.min(DEPTH_KEY_CAP), splits) {
        Some(measured) => measured / PAGE_WASTE.1 * PAGE_WASTE.0,
        None => live / RESERVED_OVER_LIVE.1 * RESERVED_OVER_LIVE.0,
    }
}

fn encoder() -> u64 {
    let windowed = 4 * plane(DIMENSION, SIDE / 4);
    let scores = 2 * HEADS as u64 * QUERIES as u64 * DEEP_TOKENS as u64 * 4;
    let deep = 4 * plane(DEEP_CHANNELS, FEATURE_SIDE);
    let neck = 2 * plane(NECK_CHANNELS, FEATURE_SIDE);
    windowed + scores + deep + neck
}

fn streaming(depth: usize, splits: usize) -> u64 {
    let mut resident = 0;
    let mut transient = 0;
    for level in 0..7 {
        let produced = production(depth, SHIFTS[level]);
        let slices = produced + RETENTION[level];
        let spans = spans(LEVEL_SIDES[level], splits) as u64;
        resident += plane(LEVEL_CHANNELS[level], LEVEL_SIDES[level]) * slices as u64 / spans;
        let window = (produced + 4) as u64;
        let working = plane(LEVEL_CHANNELS[level], LEVEL_SIDES[level]) * window / spans;
        transient = transient.max(CONV_COPIES * working);
    }
    let head_spans = spans(SIDE, splits) as u64;
    let head_window = (SLAB + 4) as u64;
    let head = 2 * CONV_COPIES * plane(2 * 32, SIDE) * head_window / head_spans;
    transient = transient.max(head);
    let features = plane(NECK_CHANNELS, FEATURE_SIDE) * (SLAB as u64 + 8);
    let emit = 2 * plane(OUTPUT_CHANNELS, SIDE) * SLAB as u64 / head_spans;
    resident + transient + features + emit
}

fn production(depth: usize, shift: usize) -> usize {
    (SLAB + shift).min(depth)
}

fn spans(side: usize, splits: usize) -> usize {
    splits.min(side / SPAN_FLOOR).max(1)
}

fn plane(channels: usize, side: usize) -> u64 {
    channels as u64 * side as u64 * side as u64 * 4
}
