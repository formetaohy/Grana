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

    pub fn coarser(self) -> Self {
        Self {
            extent: Extent::new(
                self.extent.x().div_ceil(2),
                self.extent.y().div_ceil(2),
                self.extent.z().div_ceil(2),
            ),
            spacing: Spacing::new(
                self.spacing.x() * 2.0,
                self.spacing.y() * 2.0,
                self.spacing.z() * 2.0,
            ),
        }
    }
}
