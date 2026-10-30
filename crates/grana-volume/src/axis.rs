use crate::{Extent, Spacing};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Axis {
    X,
    Y,
    Z,
}

impl Axis {
    pub const ALL: [Self; 3] = [Self::X, Self::Y, Self::Z];

    pub fn index(self) -> usize {
        match self {
            Self::X => 0,
            Self::Y => 1,
            Self::Z => 2,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::X => "X",
            Self::Y => "Y",
            Self::Z => "Z",
        }
    }

    pub fn in_plane(self) -> (Self, Self) {
        match self {
            Self::X => (Self::Y, Self::Z),
            Self::Y => (Self::X, Self::Z),
            Self::Z => (Self::X, Self::Y),
        }
    }

    pub fn extent(self, extent: Extent) -> u32 {
        match self {
            Self::X => extent.x(),
            Self::Y => extent.y(),
            Self::Z => extent.z(),
        }
    }

    pub fn spacing(self, spacing: Spacing) -> f32 {
        match self {
            Self::X => spacing.x(),
            Self::Y => spacing.y(),
            Self::Z => spacing.z(),
        }
    }
}
