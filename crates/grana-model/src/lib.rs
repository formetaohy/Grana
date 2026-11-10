#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum License {
    Commercial,
    NonCommercial,
}

impl License {
    pub fn name(self) -> &'static str {
        match self {
            Self::Commercial => "commercial",
            Self::NonCommercial => "non-commercial",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Model {
    Cpsam,
    #[default]
    CpsamV2,
}

impl Model {
    pub const ALL: [Self; 2] = [Self::CpsamV2, Self::Cpsam];

    pub fn name(self) -> &'static str {
        match self {
            Self::Cpsam => "cpsam",
            Self::CpsamV2 => "cpsam_v2",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Cpsam => "Cellpose-SAM v1",
            Self::CpsamV2 => "Cellpose-SAM v2",
        }
    }

    pub fn license(self) -> License {
        License::NonCommercial
    }

    pub fn licensed(self) -> bool {
        !cfg!(feature = "commercial")
    }

    pub fn preferred() -> Option<Self> {
        Self::ALL.into_iter().find(|model| model.licensed())
    }

    pub fn available() -> impl Iterator<Item = Self> {
        Self::ALL.into_iter().filter(|model| model.licensed())
    }

    pub fn index(self) -> u8 {
        match self {
            Self::Cpsam => 0,
            Self::CpsamV2 => 1,
        }
    }

    pub fn from_index(index: u8) -> Option<Self> {
        match index {
            0 => Some(Self::Cpsam),
            1 => Some(Self::CpsamV2),
            _ => None,
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|model| model.name() == name)
    }
}
