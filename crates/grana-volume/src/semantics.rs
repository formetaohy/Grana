#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VoxelSemantics {
    Scalar,
    Label,
    Presence,
}

impl VoxelSemantics {
    pub fn code(self) -> u8 {
        match self {
            Self::Scalar => 0,
            Self::Label => 1,
            Self::Presence => 2,
        }
    }

    pub fn averages(self) -> bool {
        matches!(self, Self::Scalar)
    }
}
