use std::sync::Mutex;

use crate::Result;
use crate::error::Error;
use crate::model::decoder::{SLAB, max_tensor_bytes};

pub const SIDE: usize = 1024;
pub const FEATURE_SIDE: usize = 64;

const DEFAULT_SPLITS: usize = 16;
const QUERIES: usize = 256;
const NECK_CHANNELS: usize = 256;
const PAGE_SLACK: u64 = 1 << 20;
const SCRATCH_PAGES: u64 = 5;
const DRIVER_OVERHEAD: u64 = 256 << 20;
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
        let peak = weights + growth(depth, DEFAULT_SPLITS, peaks);
        if peak <= spare {
            return Ok(Self {
                splits: DEFAULT_SPLITS,
                peak,
            });
        }
        Err(Error::input(format!(
            "the segmentation model needs {:.1} GiB of graphics memory for a {depth} slice volume, above the {:.1} GiB the renderer leaves free",
            peak as f64 / (1u64 << 30) as f64,
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
    match peaks.lookup(depth.min(DEPTH_KEY_CAP), splits) {
        Some(measured) => measured + DRIVER_OVERHEAD,
        None => {
            let page = max_tensor_bytes(depth, splits) + PAGE_SLACK;
            streaming_live(depth) + SCRATCH_PAGES * page + DRIVER_OVERHEAD
        }
    }
}

fn streaming_live(depth: usize) -> u64 {
    let first_slab = depth.min(SLAB);
    let keep = |consumer_end: usize| -> u64 { if consumer_end < depth { 4 } else { 0 } };
    let pass0 = plane(NECK_CHANNELS, FEATURE_SIDE) * (first_slab + 8).min(depth) as u64
        + plane(256, 64) * keep(first_slab + 6)
        + plane(256, 128) * keep(first_slab + 6)
        + plane(128, 128) * keep(first_slab + 4)
        + plane(128, 256) * keep(first_slab + 4)
        + plane(64, 256) * (first_slab + 4).min(depth) as u64
        + plane(64, 512) * (first_slab + 4).min(depth) as u64
        + plane(32, 512) * ((first_slab + 2).min(depth) + 2) as u64;
    if depth <= SLAB {
        return pass0;
    }
    let steady = plane(NECK_CHANNELS, FEATURE_SIDE) * (SLAB + 8).min(depth) as u64
        + plane(256, 64) * keep(2 * SLAB + 6)
        + plane(256, 128) * keep(2 * SLAB + 6)
        + plane(128, 128) * keep(2 * SLAB + 4)
        + plane(128, 256) * keep(2 * SLAB + 4)
        + plane(64, 256) * (SLAB + 4).min(depth) as u64
        + plane(64, 512) * (SLAB + 6).min(depth) as u64
        + plane(32, 512) * (4 + SLAB + 2).min(depth + 2) as u64;
    pass0.max(steady)
}

fn plane(channels: usize, side: usize) -> u64 {
    channels as u64 * side as u64 * side as u64 * 4
}
