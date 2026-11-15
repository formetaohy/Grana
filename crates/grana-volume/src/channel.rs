use crate::{Intensity, VoxelFormat};

const BITS: usize = crate::CHANNEL_LIMIT;

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

#[derive(Clone, Debug, PartialEq)]
pub struct ChannelInfo {
    name: String,
    tint: [f32; 3],
    intensity: Intensity,
    window: Intensity,
    format: VoxelFormat,
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
        }
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
}
