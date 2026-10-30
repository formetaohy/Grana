use std::ops::Range;

use crate::{Extent, VoxelData};

pub const BRICK_SIDE: u32 = 32;
pub const BRICK_APRON: u32 = 1;
pub const PADDED_SIDE: u32 = BRICK_SIDE + 2 * BRICK_APRON;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BrickCoord {
    x: u32,
    y: u32,
    z: u32,
}

impl BrickCoord {
    pub fn new(x: u32, y: u32, z: u32) -> Self {
        Self { x, y, z }
    }

    pub fn x(self) -> u32 {
        self.x
    }

    pub fn y(self) -> u32 {
        self.y
    }

    pub fn z(self) -> u32 {
        self.z
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BrickGrid {
    extent: Extent,
    counts: [u32; 3],
}

impl BrickGrid {
    pub fn new(extent: Extent) -> Self {
        let counts =
            [extent.x(), extent.y(), extent.z()].map(|dimension| dimension.div_ceil(BRICK_SIDE));
        Self { extent, counts }
    }

    pub fn extent(self) -> Extent {
        self.extent
    }

    pub fn counts(self) -> [u32; 3] {
        self.counts
    }

    pub fn count(self) -> usize {
        self.counts.iter().map(|count| *count as usize).product()
    }

    pub fn index(self, coord: BrickCoord) -> usize {
        assert!(
            coord.x < self.counts[0] && coord.y < self.counts[1] && coord.z < self.counts[2],
            "brick coordinate out of bounds"
        );
        (coord.x + self.counts[0] * (coord.y + self.counts[1] * coord.z)) as usize
    }

    pub fn origin(self, coord: BrickCoord) -> [u32; 3] {
        [
            coord.x * BRICK_SIDE,
            coord.y * BRICK_SIDE,
            coord.z * BRICK_SIDE,
        ]
    }

    pub fn extent_of(self, coord: BrickCoord) -> Extent {
        let origin = self.origin(coord);
        Extent::new(
            (self.extent.x() - origin[0]).min(BRICK_SIDE),
            (self.extent.y() - origin[1]).min(BRICK_SIDE),
            (self.extent.z() - origin[2]).min(BRICK_SIDE),
        )
    }

    pub fn range(self, coord: BrickCoord) -> [Range<u32>; 3] {
        let origin = self.origin(coord);
        let extent = self.extent_of(coord);
        [
            origin[0]..origin[0] + extent.x(),
            origin[1]..origin[1] + extent.y(),
            origin[2]..origin[2] + extent.z(),
        ]
    }

    pub fn padded_range(self, coord: BrickCoord) -> [Range<u32>; 3] {
        let mut range = self.range(coord);
        for (axis, limit) in [self.extent.x(), self.extent.y(), self.extent.z()]
            .into_iter()
            .enumerate()
        {
            range[axis].start = range[axis].start.saturating_sub(BRICK_APRON);
            range[axis].end = (range[axis].end + BRICK_APRON).min(limit);
        }
        range
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct BrickData {
    channels: Vec<VoxelData>,
}

impl BrickData {
    pub fn new(channels: Vec<VoxelData>) -> Self {
        assert!(!channels.is_empty(), "a brick holds at least one channel");
        let side = PADDED_SIDE as u64;
        assert!(
            channels
                .iter()
                .all(|channel| channel.voxel_count() as u64 == side * side * side),
            "every brick channel holds a padded cube"
        );
        Self { channels }
    }

    pub fn channels(&self) -> &[VoxelData] {
        &self.channels
    }
}

pub fn pad_brick(
    source: &VoxelData,
    source_origin: [u32; 3],
    source_extent: Extent,
    brick_origin: [u32; 3],
    volume_extent: Extent,
) -> VoxelData {
    match source {
        VoxelData::U8(data) => VoxelData::U8(pad(
            data,
            source_origin,
            source_extent,
            brick_origin,
            volume_extent,
        )),
        VoxelData::U16(data) => VoxelData::U16(pad(
            data,
            source_origin,
            source_extent,
            brick_origin,
            volume_extent,
        )),
        VoxelData::F32(data) => VoxelData::F32(pad(
            data,
            source_origin,
            source_extent,
            brick_origin,
            volume_extent,
        )),
    }
}

fn pad<T: Copy>(
    source: &[T],
    source_origin: [u32; 3],
    source_extent: Extent,
    brick_origin: [u32; 3],
    volume_extent: Extent,
) -> Vec<T> {
    assert_eq!(
        source.len() as u64,
        source_extent.voxel_count(),
        "the source voxels must match the source extent"
    );
    let side = PADDED_SIDE as usize;
    let mut padded = Vec::with_capacity(side * side * side);
    for z in 0..PADDED_SIDE {
        let vz = clamp(volume_extent.z(), brick_origin[2], z);
        let lz = (vz - source_origin[2]) as usize;
        assert!(
            lz < source_extent.z() as usize,
            "the source covers the brick"
        );
        for y in 0..PADDED_SIDE {
            let vy = clamp(volume_extent.y(), brick_origin[1], y);
            let ly = (vy - source_origin[1]) as usize;
            assert!(
                ly < source_extent.y() as usize,
                "the source covers the brick"
            );
            for x in 0..PADDED_SIDE {
                let vx = clamp(volume_extent.x(), brick_origin[0], x);
                let lx = (vx - source_origin[0]) as usize;
                assert!(
                    lx < source_extent.x() as usize,
                    "the source covers the brick"
                );
                let index =
                    lx + source_extent.x() as usize * (ly + source_extent.y() as usize * lz);
                padded.push(source[index]);
            }
        }
    }
    padded
}

fn clamp(limit: u32, origin: u32, offset: u32) -> u32 {
    let value = origin as i64 + offset as i64 - BRICK_APRON as i64;
    value.clamp(0, limit as i64 - 1) as u32
}
