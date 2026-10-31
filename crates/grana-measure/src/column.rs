#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    Count,
    Second,
    Micrometer,
    SquareMicrometer,
    CubicMicrometer,
    MicrometerPerSecond,
}

impl Unit {
    pub fn suffix(self) -> &'static str {
        match self {
            Self::Count => "",
            Self::Second => "s",
            Self::Micrometer => "µm",
            Self::SquareMicrometer => "µm²",
            Self::CubicMicrometer => "µm³",
            Self::MicrometerPerSecond => "µm/s",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Column {
    label: String,
    key: String,
    unit: Unit,
    decimals: u8,
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
        }
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
