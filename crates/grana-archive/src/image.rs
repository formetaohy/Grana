use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use grana_volume::{Intensity, Level};

pub struct Channel<'a> {
    pub name: &'a str,
    pub tint: [f32; 3],
    pub range: Intensity,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sample {
    Float,
    Unsigned16,
    Unsigned8,
}

pub type Filler<'a> = dyn Fn(usize, u32, usize, [u32; 3], [u32; 3], &mut [f32]) -> Result<(), String>
    + Send
    + Sync
    + 'a;

#[derive(Clone, Copy)]
pub enum Fill<'a> {
    EveryLevel(&'a Filler<'a>),
    FinestLevel(&'a Filler<'a>),
}

impl<'a> Fill<'a> {
    pub fn filler(self) -> &'a Filler<'a> {
        match self {
            Self::EveryLevel(filler) | Self::FinestLevel(filler) => filler,
        }
    }

    pub fn derived(self) -> bool {
        matches!(self, Self::FinestLevel(_))
    }
}

pub struct ImageSpec<'a> {
    pub name: &'a str,
    pub channels: &'a [Channel<'a>],
    pub levels: &'a [Level],
    pub placement: [f32; 3],
    pub frames: u32,
    pub interval: Option<f32>,
    pub sample: Sample,
    pub fill: Fill<'a>,
    pub cancel: &'a AtomicBool,
    pub progress: &'a mut dyn FnMut(usize, usize),
}

impl ImageSpec<'_> {
    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    pub fn check(&self) {
        assert!(
            !self.name.trim().is_empty(),
            "an exported image carries a name"
        );
        assert!(
            !self.channels.is_empty(),
            "an exported image carries at least one channel"
        );
        assert!(
            self.channels
                .iter()
                .all(|channel| !channel.name.trim().is_empty()),
            "an exported channel carries a name"
        );
        assert!(
            !self.levels.is_empty(),
            "an exported image carries at least one resolution level"
        );
        assert!(
            self.frames > 0,
            "an exported image carries at least one frame"
        );
        assert!(
            self.interval
                .is_none_or(|interval| interval.is_finite() && interval > 0.0),
            "an exported image carries a positive frame interval"
        );
        assert!(
            self.frames == 1 || self.interval.is_some(),
            "an exported time series declares its frame interval"
        );
        assert!(
            self.placement.iter().all(|value| value.is_finite()),
            "an exported image is placed at a finite origin"
        );
    }
}

pub struct Written {
    path: PathBuf,
    name: String,
    channels: usize,
    frames: u32,
    levels: usize,
    bytes: u64,
}

impl Written {
    pub fn new(
        path: PathBuf,
        name: String,
        channels: usize,
        frames: u32,
        levels: usize,
        bytes: u64,
    ) -> Self {
        Self {
            path,
            name,
            channels,
            frames,
            levels,
            bytes,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn channels(&self) -> usize {
        self.channels
    }

    pub fn frames(&self) -> u32 {
        self.frames
    }

    pub fn levels(&self) -> usize {
        self.levels
    }

    pub fn bytes(&self) -> u64 {
        self.bytes
    }
}
