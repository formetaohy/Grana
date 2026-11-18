pub(crate) const SURFACE_PLANE_BYTES_PER_VOXEL: u64 = 8;

pub(crate) const SURFACE_ARRAY_BYTES_PER_VOXEL: u64 = 17;

pub(crate) const SURFACE_FIELD_BYTES_PER_VOXEL: u64 = 4;

pub(crate) const SURFACE_FRAGMENT_BYTES: u64 = 96 << 20;

pub(crate) const FIELD_BLOCK_VOXELS: u64 = 512;

pub(crate) const FIELD_BIN_BYTES: u64 = 2;

pub(crate) const FIELD_BIN_MIN: u64 = 16;

pub(crate) const FIELD_BIN_MAX: u64 = 128;

pub(crate) const FIELD_COUNT_BYTES: u64 = 4;

pub(crate) const FIELD_LEVEL_BYTES: u64 = 12;

pub(crate) const FIELD_BACKGROUND_BYTES: u64 = 4;

pub(crate) const FIELD_SIGNAL_BYTES: u64 = 4;

pub(crate) const CELL_VOXELS: u64 = 128 << 20;

pub(crate) const CELL_BAND_BYTES: u64 = 512 << 20;

pub(crate) const CELL_VOXEL_BYTES: u64 = 16;

pub(crate) const COLOC_BALANCED_VOXELS: u64 = 1 << 22;

pub(crate) const COLOC_FAST_VOXELS: u64 = 1 << 20;

pub(crate) const OBJECT_COLOC_VOXELS: u64 = 128 << 20;

pub(crate) const LOCALIZATION_PREVIEW_BYTES: u64 = 256 << 20;
