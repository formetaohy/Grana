#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Calibration {
    Recorded,
    Inferred,
    Assumed,
    Uncalibrated,
}

impl Calibration {
    pub fn recorded(self) -> bool {
        self == Self::Recorded
    }

    pub fn measured(self) -> bool {
        matches!(self, Self::Recorded | Self::Inferred | Self::Assumed)
    }

    pub fn index(self) -> u8 {
        match self {
            Self::Recorded => 0,
            Self::Inferred => 1,
            Self::Assumed => 2,
            Self::Uncalibrated => 3,
        }
    }

    pub fn from_index(index: u8) -> Option<Self> {
        match index {
            0 => Some(Self::Recorded),
            1 => Some(Self::Inferred),
            2 => Some(Self::Assumed),
            3 => Some(Self::Uncalibrated),
            _ => None,
        }
    }
}
