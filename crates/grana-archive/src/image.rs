use std::path::{Path, PathBuf};

use grana_volume::{Intensity, Level};

pub struct Channel<'a> {
    pub name: &'a str,
    pub tint: [f32; 3],
    pub range: Intensity,
}

pub type Fill<'a> =
    dyn FnMut(usize, u32, usize, [u32; 3], [u32; 3], &mut [f32]) -> Result<(), String> + 'a;

pub struct ImageSpec<'a> {
    pub name: &'a str,
    pub channels: &'a [Channel<'a>],
    pub levels: &'a [Level],
    pub placement: [f32; 3],
    pub frames: u32,
    pub interval: f32,
    pub fill: &'a mut Fill<'a>,
    pub progress: &'a mut dyn FnMut(usize, usize),
}

impl ImageSpec<'_> {
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
            self.interval.is_finite() && self.interval > 0.0,
            "an exported image carries a positive frame interval"
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
