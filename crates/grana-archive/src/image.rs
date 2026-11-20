use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use grana_volume::{Calibration, Intensity, Level, VoxelData, VoxelFormat};

pub struct Channel<'a> {
    pub name: &'a str,
    pub tint: [f32; 3],
    pub intensity: Intensity,
}

#[derive(Clone, Copy)]
pub struct Block {
    pub level: usize,
    pub frame: u32,
    pub channel: usize,
    pub origin: [u32; 3],
    pub extent: [u32; 3],
}

impl Block {
    pub fn voxels(self) -> usize {
        (self.extent[0] as usize) * (self.extent[1] as usize) * (self.extent[2] as usize)
    }

    pub fn check(self, format: VoxelFormat, data: &VoxelData) {
        assert_eq!(
            data.format().union(format),
            Some(format),
            "an exported block widens the sample type"
        );
        assert_eq!(
            data.voxel_count(),
            self.voxels(),
            "an exported block covers every voxel of its extent"
        );
    }
}

pub type Filler<'a> = dyn Fn(Block) -> Result<VoxelData, String> + Send + Sync + 'a;

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
    pub sample: VoxelFormat,
    pub calibration: Calibration,
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
            self.channels
                .iter()
                .all(|channel| channel.intensity.span() > 0.0),
            "an exported channel carries a display window with a positive span"
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
