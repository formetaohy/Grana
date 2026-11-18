use grana_volume::Level;

use crate::shape;

const SHARD_OVERHEAD: u64 = 8 << 10;
const COMPRESSED_EXPANSION: u64 = 128;
const METADATA: u64 = 1 << 20;

pub fn image_space(levels: &[Level], frames: u32, channels: usize) -> u64 {
    planned(levels, u64::from(frames), channels as u64, 4)
}

pub fn label_space(levels: &[Level], frames: usize) -> u64 {
    planned(levels, frames as u64, 1, 2)
}

fn planned(levels: &[Level], frames: u64, channels: u64, bytes_per_voxel: u64) -> u64 {
    let mut raw = 0u64;
    let mut shards = 0u64;
    for level in levels {
        let extent = shape::extent(*level);
        raw += shape::voxels(extent) * frames * channels * bytes_per_voxel;
        shards += shape::count(extent) * frames * channels;
    }
    raw + raw / COMPRESSED_EXPANSION + shards * SHARD_OVERHEAD + METADATA
}
