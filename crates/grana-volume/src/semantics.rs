#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VoxelSemantics {
    Scalar,
    Label,
    Presence,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Reduction {
    Average,
    Dominant,
    Presence,
}

impl Reduction {
    pub fn averages(self) -> bool {
        matches!(self, Self::Average)
    }
}

impl VoxelSemantics {
    pub fn code(self) -> u8 {
        match self {
            Self::Scalar => 0,
            Self::Label => 1,
            Self::Presence => 2,
        }
    }

    pub fn reduction(self) -> Reduction {
        match self {
            Self::Scalar => Reduction::Average,
            Self::Label => Reduction::Dominant,
            Self::Presence => Reduction::Presence,
        }
    }
}
