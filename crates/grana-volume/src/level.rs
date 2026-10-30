use crate::{Extent, VoxelData};

#[derive(Clone, Debug, PartialEq)]
pub struct Level {
    extent: Extent,
    data: VoxelData,
}

impl Level {
    pub fn new(extent: Extent, data: VoxelData) -> Self {
        assert_eq!(
            data.voxel_count() as u64,
            extent.voxel_count(),
            "voxel count must match extent"
        );
        Self { extent, data }
    }

    pub fn extent(&self) -> Extent {
        self.extent
    }

    pub fn data(&self) -> &VoxelData {
        &self.data
    }

    pub fn downsampled(&self) -> Self {
        Self {
            extent: self.extent.halve(),
            data: self.data.downsampled(self.extent),
        }
    }
}
