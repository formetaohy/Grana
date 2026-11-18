use grana_volume::Level;

const CHUNK_X: u32 = 256;
const CHUNK_Y: u32 = 256;
const CHUNK_Z: u32 = 32;
const CHUNK_OVERHEAD: u64 = 8 << 10;
const COMPRESSED_EXPANSION: u64 = 128;
const METADATA: u64 = 1 << 20;

pub fn image_space(levels: &[Level], frames: u32, channels: usize) -> u64 {
    planned(levels, u64::from(frames) * channels as u64, 4)
}

pub fn label_space(levels: &[Level], frames: usize) -> u64 {
    planned(levels, frames as u64, 2)
}

fn planned(levels: &[Level], copies: u64, bytes_per_voxel: u64) -> u64 {
    let mut raw = 0u64;
    let mut chunks = 0u64;
    for level in levels {
        let extent = level.extent();
        let voxels = u64::from(extent.x()) * u64::from(extent.y()) * u64::from(extent.z());
        let cells = count(extent.x(), extent.x().min(CHUNK_X)) as u64
            * count(extent.y(), extent.y().min(CHUNK_Y)) as u64
            * count(extent.z(), extent.z().min(CHUNK_Z)) as u64;
        raw += voxels * copies * bytes_per_voxel;
        chunks += cells * copies;
    }
    raw + raw / COMPRESSED_EXPANSION + chunks * CHUNK_OVERHEAD + METADATA
}

fn count(extent: u32, chunk: u32) -> u32 {
    extent.div_ceil(chunk.max(1))
}
