use sha2::{Digest, Sha256};

use grana_volume::{Fingerprint, Level, Pyramid, VoxelData, VoxelFormat};

use crate::{Error, Image};

const SAMPLE_CHANNELS: usize = 4;
const SAMPLE_PATCH: u32 = 64;

impl Image {
    pub fn fingerprint(&self) -> Result<Fingerprint, Error> {
        let pyramid = self.pyramid();
        let level = pyramid.level(0);
        let mut hasher = Sha256::new();
        digest_pyramid(&mut hasher, pyramid);
        for channel in sampled_channels(pyramid.channel_count()) {
            for frame in sampled_frames(pyramid.frame_count()) {
                digest_patch(&mut hasher, self, level, frame, channel)?;
            }
        }
        Ok(Fingerprint::new(hasher.finalize().into()))
    }
}

fn digest_pyramid(hasher: &mut Sha256, pyramid: &Pyramid) {
    integer(hasher, u64::from(pyramid.frame_count()));
    integer(hasher, u64::from(pyramid.frame_interval().to_bits()));
    integer(hasher, pyramid.stored() as u64);
    for level in pyramid.levels() {
        let extent = level.extent();
        for value in [extent.x(), extent.y(), extent.z()] {
            integer(hasher, u64::from(value));
        }
        let spacing = level.spacing();
        for value in [spacing.x(), spacing.y(), spacing.z()] {
            integer(hasher, u64::from(value.to_bits()));
        }
        for value in level.origin() {
            integer(hasher, u64::from(value.to_bits()));
        }
    }
    for value in pyramid.placement() {
        integer(hasher, u64::from(value.to_bits()));
    }
    for channel in pyramid.channels() {
        integer(hasher, channel.name().len() as u64);
        hasher.update(channel.name().as_bytes());
        for value in channel.tint() {
            integer(hasher, u64::from(value.to_bits()));
        }
        for value in [
            channel.intensity().min(),
            channel.intensity().max(),
            channel.window().min(),
            channel.window().max(),
        ] {
            integer(hasher, u64::from(value.to_bits()));
        }
        integer(
            hasher,
            match channel.format() {
                VoxelFormat::U8 => 0,
                VoxelFormat::U16 => 1,
                VoxelFormat::F32 => 2,
            },
        );
    }
}

fn digest_patch(
    hasher: &mut Sha256,
    image: &Image,
    level: Level,
    frame: u32,
    channel: usize,
) -> Result<(), Error> {
    let extent = level.extent();
    let ranges = [
        patch(extent.x()),
        patch(extent.y()),
        (extent.z() / 2)..(extent.z() / 2 + 1),
    ];
    integer(hasher, u64::from(channel as u32));
    integer(hasher, u64::from(frame));
    for range in &ranges {
        integer(hasher, u64::from(range.start));
        integer(hasher, u64::from(range.end));
    }
    let data = image.read_region(frame, 0, channel, &ranges, [0.0; 3])?;
    digest_values(hasher, &data);
    Ok(())
}

fn digest_values(hasher: &mut Sha256, data: &VoxelData) {
    for index in 0..data.voxel_count() {
        integer(hasher, u64::from(data.value(index).to_bits()));
    }
}

fn patch(limit: u32) -> std::ops::Range<u32> {
    let side = SAMPLE_PATCH.min(limit).max(1);
    let start = (limit - side) / 2;
    start..(start + side)
}

fn sampled_channels(count: usize) -> Vec<usize> {
    if count <= SAMPLE_CHANNELS {
        return (0..count).collect();
    }
    let mut picked = vec![0, count / 3, 2 * count / 3, count - 1];
    picked.sort_unstable();
    picked.dedup();
    picked
}

fn sampled_frames(count: u32) -> Vec<u32> {
    if count <= 1 {
        return vec![0];
    }
    vec![0, count - 1]
}

fn integer(hasher: &mut Sha256, value: u64) {
    hasher.update(value.to_le_bytes());
}
