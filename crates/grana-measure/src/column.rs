use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Unit {
    Count,
    Percent,
    Second,
    Pixel,
    SquarePixel,
    CubicPixel,
    PixelPerSecond,
    Micrometer,
    SquareMicrometer,
    CubicMicrometer,
    MicrometerPerSecond,
}

impl Unit {
    pub fn suffix(self) -> &'static str {
        match self {
            Self::Count => "",
            Self::Percent => "%",
            Self::Second => "s",
            Self::Pixel => "px",
            Self::SquarePixel => "px²",
            Self::CubicPixel => "px³",
            Self::PixelPerSecond => "px/s",
            Self::Micrometer => "µm",
            Self::SquareMicrometer => "µm²",
            Self::CubicMicrometer => "µm³",
            Self::MicrometerPerSecond => "µm/s",
        }
    }

    pub fn physical(self) -> bool {
        matches!(
            self,
            Self::Micrometer
                | Self::SquareMicrometer
                | Self::CubicMicrometer
                | Self::MicrometerPerSecond
        )
    }

    pub fn spatial(self) -> bool {
        matches!(
            self,
            Self::Pixel
                | Self::SquarePixel
                | Self::CubicPixel
                | Self::PixelPerSecond
                | Self::Micrometer
                | Self::SquareMicrometer
                | Self::CubicMicrometer
                | Self::MicrometerPerSecond
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Completeness {
    Always,
    Possibly,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Column {
    label: String,
    key: String,
    unit: Unit,
    decimals: u8,
    completeness: Completeness,
}

impl Column {
    pub(crate) fn new(
        label: impl Into<String>,
        key: impl Into<String>,
        unit: Unit,
        decimals: u8,
    ) -> Self {
        assert!(decimals <= 6, "a column keeps a bounded number of decimals");
        Self {
            label: label.into(),
            key: key.into(),
            unit,
            decimals,
            completeness: Completeness::Always,
        }
    }

    pub(crate) fn possibly_absent(mut self) -> Self {
        self.completeness = Completeness::Possibly;
        self
    }

    pub fn completeness(&self) -> Completeness {
        self.completeness
    }

    pub fn complete(&self) -> bool {
        self.completeness == Completeness::Always
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn key(&self) -> &str {
        &self.key
    }

    pub fn unit(&self) -> Unit {
        self.unit
    }

    pub fn decimals(&self) -> u8 {
        self.decimals
    }

    pub fn format(&self, value: f32) -> String {
        format!("{:.*}", self.decimals as usize, value)
    }
}
