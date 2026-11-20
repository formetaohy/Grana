use crate::{Intensity, VoxelFormat};

const BITS: usize = crate::CHANNEL_LIMIT;

const TINTS: [[f32; 3]; 8] = [
    [0.25, 0.55, 1.00],
    [0.20, 0.95, 0.35],
    [1.00, 0.25, 0.55],
    [1.00, 0.72, 0.20],
    [0.60, 0.40, 1.00],
    [0.20, 0.90, 0.90],
    [1.00, 0.45, 0.20],
    [0.85, 0.85, 0.30],
];

pub fn tint_of(channel: u32) -> [f32; 3] {
    TINTS[channel as usize % TINTS.len()]
}

pub fn name_of(channel: u32) -> String {
    format!("Channel {}", channel + 1)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ChannelSet {
    bits: u32,
}

impl ChannelSet {
    pub const NONE: Self = Self { bits: 0 };

    pub const ALL: Self = Self { bits: u32::MAX };

    pub fn of(channel: usize) -> Self {
        assert!(channel < BITS, "a channel set names a dataset channel");
        Self { bits: 1 << channel }
    }

    pub fn with(self, channel: usize) -> Self {
        self.union(Self::of(channel))
    }

    pub fn union(self, other: Self) -> Self {
        Self {
            bits: self.bits | other.bits,
        }
    }

    pub fn contains(self, channel: usize) -> bool {
        channel < BITS && self.bits & (1 << channel) != 0
    }

    pub fn is_empty(self) -> bool {
        self.bits == 0
    }

    pub fn covers(self, channels: usize) -> bool {
        assert!(
            channels <= BITS,
            "a dataset holds at most {BITS} display channels"
        );
        (0..channels).all(|channel| self.contains(channel))
    }

    pub fn iter(self) -> impl Iterator<Item = usize> {
        (0..BITS).filter(move |channel| self.contains(*channel))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Sampling {
    #[default]
    Mean,
    Nearest,
}

impl Sampling {
    pub fn code(self) -> u8 {
        match self {
            Self::Mean => 0,
            Self::Nearest => 1,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ChannelInfo {
    name: String,
    tint: [f32; 3],
    intensity: Intensity,
    window: Intensity,
    format: VoxelFormat,
    sampling: Sampling,
}

impl ChannelInfo {
    pub fn new(
        name: impl Into<String>,
        tint: [f32; 3],
        intensity: Intensity,
        window: Intensity,
        format: VoxelFormat,
    ) -> Self {
        assert!(
            tint.iter().all(|component| (0.0..=1.0).contains(component)),
            "a channel tint is an srgb triple"
        );
        assert!(
            window.span() > 0.0,
            "a display window needs a positive span"
        );
        Self {
            name: name.into(),
            tint,
            intensity,
            window,
            format,
            sampling: Sampling::Mean,
        }
    }

    pub fn with_sampling(mut self, sampling: Sampling) -> Self {
        self.sampling = sampling;
        self
    }

    pub fn resolve(
        name: impl Into<String>,
        tint: [f32; 3],
        format: VoxelFormat,
        domains: &[Option<Intensity>],
        windows: &[Option<Intensity>],
    ) -> Self {
        let intensity = format
            .domain()
            .or_else(|| first(domains))
            .unwrap_or_else(fallback);
        let window = first(windows).unwrap_or(intensity);
        Self::new(name, tint, intensity, window, format)
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn tint(&self) -> [f32; 3] {
        self.tint
    }

    pub fn intensity(&self) -> Intensity {
        self.intensity
    }

    pub fn window(&self) -> Intensity {
        self.window
    }

    pub fn format(&self) -> VoxelFormat {
        self.format
    }

    pub fn sampling(&self) -> Sampling {
        self.sampling
    }
}

fn first(candidates: &[Option<Intensity>]) -> Option<Intensity> {
    candidates
        .iter()
        .flatten()
        .copied()
        .find(|candidate| candidate.span() > 0.0)
}

fn fallback() -> Intensity {
    Intensity::new(0.0, 1.0)
}
