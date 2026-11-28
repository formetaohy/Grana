use crate::Result;
use crate::error::Error;
use crate::model::decoder::{residency, workspace};

pub const SIDE: usize = 1024;

const DEFAULT_SPLITS: usize = 16;
const QUERIES: usize = 256;
const DRIVER_OVERHEAD: u64 = 128 << 20;

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

    pub(crate) fn of(weights: u64, depth: usize, spare: u64) -> Result<Self> {
        let peak = Self::estimate(weights, depth, DEFAULT_SPLITS);
        if peak > spare {
            return Err(Error::input(format!(
                "the segmentation model needs {:.1} GiB of graphics memory for a {depth} slice region, above the {:.1} GiB the renderer leaves free; segment a shallower region",
                peak as f64 / (1u64 << 30) as f64,
                spare as f64 / (1u64 << 30) as f64,
            )));
        }
        Ok(Self {
            splits: DEFAULT_SPLITS,
            peak,
        })
    }

    #[doc(hidden)]
    pub fn forced(splits: usize, peak: u64) -> Self {
        Self { splits, peak }
    }

    pub(crate) fn estimate(weights: u64, depth: usize, splits: usize) -> u64 {
        weights + residency(depth) + workspace(depth, splits) + DRIVER_OVERHEAD
    }
}
