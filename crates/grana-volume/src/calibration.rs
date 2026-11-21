#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Calibration {
    Recorded,
    Inferred,
    Assumed,
    Declared,
    Uncalibrated,
}

impl Calibration {
    pub fn recorded(self) -> bool {
        self == Self::Recorded
    }

    pub fn measured(self) -> bool {
        matches!(
            self,
            Self::Recorded | Self::Inferred | Self::Assumed | Self::Declared
        )
    }

    pub fn asserted(self) -> bool {
        self == Self::Declared
    }

    pub fn index(self) -> u8 {
        match self {
            Self::Recorded => 0,
            Self::Inferred => 1,
            Self::Assumed => 2,
            Self::Declared => 3,
            Self::Uncalibrated => 4,
        }
    }

    pub fn from_index(index: u8) -> Option<Self> {
        match index {
            0 => Some(Self::Recorded),
            1 => Some(Self::Inferred),
            2 => Some(Self::Assumed),
            3 => Some(Self::Declared),
            4 => Some(Self::Uncalibrated),
            _ => None,
        }
    }
}
