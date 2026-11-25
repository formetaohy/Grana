use grana_volume::VoxelData;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Key {
    pub(crate) namespace: u64,
    pub(crate) level: u32,
    pub(crate) time: u32,
    pub(crate) channel: u32,
    pub(crate) z: u32,
}

impl Key {
    pub(crate) fn with_level(self, level: u32) -> Self {
        Self { level, ..self }
    }

    pub(crate) fn with_z(self, z: u32) -> Self {
        Self { z, ..self }
    }
}

pub(crate) struct Plane {
    width: u32,
    height: u32,
    data: VoxelData,
}

impl Plane {
    pub(crate) fn new(width: u32, height: u32, data: VoxelData) -> Self {
        assert_eq!(
            data.voxel_count() as u64,
            u64::from(width) * u64::from(height),
            "a plane holds one channel of a whole slice"
        );
        Self {
            width,
            height,
            data,
        }
    }

    pub(crate) fn width(&self) -> u32 {
        self.width
    }

    pub(crate) fn height(&self) -> u32 {
        self.height
    }

    pub(crate) fn data(&self) -> &VoxelData {
        &self.data
    }

    pub(crate) fn bytes(&self) -> u64 {
        self.data.bytes() + size_of::<Self>() as u64
    }
}
