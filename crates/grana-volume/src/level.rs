use crate::{Extent, Spacing};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Level {
    extent: Extent,
    spacing: Spacing,
    origin: [f32; 3],
}

impl Level {
    pub fn new(extent: Extent, spacing: Spacing) -> Self {
        let world = spacing.world_extent(extent);
        Self::placed(
            extent,
            spacing,
            [-0.5 * world[0], -0.5 * world[1], -0.5 * world[2]],
        )
    }

    pub fn placed(extent: Extent, spacing: Spacing, origin: [f32; 3]) -> Self {
        assert!(
            origin.iter().all(|value| value.is_finite()),
            "a level origin is finite"
        );
        Self {
            extent,
            spacing,
            origin,
        }
    }

    pub fn extent(self) -> Extent {
        self.extent
    }

    pub fn spacing(self) -> Spacing {
        self.spacing
    }

    pub fn origin(self) -> [f32; 3] {
        self.origin
    }

    pub fn center(self) -> [f32; 3] {
        let world = self.world_extent();
        [
            self.origin[0] + 0.5 * world[0],
            self.origin[1] + 0.5 * world[1],
            self.origin[2] + 0.5 * world[2],
        ]
    }

    pub fn world_extent(self) -> [f32; 3] {
        self.spacing.world_extent(self.extent)
    }

    pub fn voxel(self) -> [f32; 3] {
        [self.spacing.x(), self.spacing.y(), self.spacing.z()]
    }

    pub fn voxel_center(self, index: [f32; 3]) -> [f32; 3] {
        let voxel = self.voxel();
        [
            self.origin[0] + (index[0] + 0.5) * voxel[0],
            self.origin[1] + (index[1] + 0.5) * voxel[1],
            self.origin[2] + (index[2] + 0.5) * voxel[2],
        ]
    }

    pub fn voxel_of(self, point: [f32; 3]) -> [f32; 3] {
        let voxel = self.voxel();
        [
            (point[0] - self.origin[0]) / voxel[0],
            (point[1] - self.origin[1]) / voxel[1],
            (point[2] - self.origin[2]) / voxel[2],
        ]
    }

    pub fn coarser(self) -> Self {
        Self::placed(
            Extent::new(
                self.extent.x().div_ceil(2),
                self.extent.y().div_ceil(2),
                self.extent.z().div_ceil(2),
            ),
            Spacing::new(
                self.spacing.x() * 2.0,
                self.spacing.y() * 2.0,
                self.spacing.z() * 2.0,
            ),
            self.origin,
        )
    }
}
