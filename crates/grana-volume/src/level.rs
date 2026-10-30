use crate::{Extent, Spacing};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Level {
    extent: Extent,
    spacing: Spacing,
}

impl Level {
    pub fn new(extent: Extent, spacing: Spacing) -> Self {
        Self { extent, spacing }
    }

    pub fn extent(self) -> Extent {
        self.extent
    }

    pub fn spacing(self) -> Spacing {
        self.spacing
    }

    pub fn world_extent(self) -> [f32; 3] {
        self.spacing.world_extent(self.extent)
    }
}
