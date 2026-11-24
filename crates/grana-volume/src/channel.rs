use crate::{Intensity, NoData, Statistics, VoxelFormat, VoxelSemantics};

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

#[derive(Clone, Debug, PartialEq)]
pub struct ChannelInfo {
    name: String,
    tint: [f32; 3],
    statistics: Statistics,
    format: VoxelFormat,
    window: Option<Intensity>,
    no_data: Option<NoData>,
}

impl ChannelInfo {
    pub fn new(
        name: impl Into<String>,
        tint: [f32; 3],
        statistics: Statistics,
        format: VoxelFormat,
    ) -> Self {
        assert!(
            tint.iter().all(|component| (0.0..=1.0).contains(component)),
            "a channel tint is an srgb triple"
        );
        Self {
            name: name.into(),
            tint,
            statistics,
            format,
            window: None,
            no_data: None,
        }
    }

    pub fn with_window(mut self, window: Intensity) -> Self {
        assert!(
            window.span() > 0.0,
            "a channel display window spans a positive intensity"
        );
        self.window = Some(window);
        self
    }

    pub fn with_no_data(mut self, no_data: NoData) -> Self {
        assert_eq!(
            no_data.format(),
            self.format,
            "a channel no-data value holds the format of its volume"
        );
        self.no_data = no_data.declared();
        self
    }

    pub fn no_data(&self) -> Option<NoData> {
        self.no_data
    }

    pub fn fill(&self) -> NoData {
        self.no_data.unwrap_or_else(|| NoData::zero(self.format))
    }

    pub fn window(&self) -> Option<Intensity> {
        self.window
    }

    pub fn display(&self) -> Intensity {
        self.window.unwrap_or(self.statistics.intensity())
    }

    pub fn declared(
        name: impl Into<String>,
        tint: [f32; 3],
        intensity: Intensity,
        format: VoxelFormat,
    ) -> Self {
        Self::new(
            name,
            tint,
            Statistics::declared(intensity, VoxelSemantics::Scalar),
            format,
        )
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn tint(&self) -> [f32; 3] {
        self.tint
    }

    pub fn statistics(&self) -> Statistics {
        self.statistics
    }

    pub fn intensity(&self) -> Intensity {
        self.statistics.intensity()
    }

    pub fn semantics(&self) -> VoxelSemantics {
        self.statistics.semantics()
    }

    pub fn with_statistics(mut self, statistics: Statistics) -> Self {
        self.statistics = statistics;
        self
    }

    pub fn format(&self) -> VoxelFormat {
        self.format
    }
}
