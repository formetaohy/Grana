use std::ops::Range;
use std::sync::Arc;

use grana_image::{Format, Image, Labels};
use grana_object::Mask;
use grana_scene::{MaskChannel, Scene};
use grana_volume::{BrickCoord, BrickData, BrickGrid, PADDED_SIDE, Pyramid, VoxelData};

#[derive(Debug, thiserror::Error)]
pub enum ChannelsError {
    #[error(transparent)]
    Image(#[from] grana_image::Error),
    #[error("the derived channel cannot be read: {detail}")]
    Derived { detail: String },
}

impl From<ChannelsError> for String {
    fn from(error: ChannelsError) -> Self {
        error.to_string()
    }
}

pub struct Channels {
    reader: Arc<Image>,
    masks: Vec<Arc<MaskChannel>>,
    pyramid: Pyramid,
}

impl Channels {
    pub fn new(reader: Arc<Image>, scene: &Scene) -> Self {
        Self {
            masks: scene.mask_channels().to_vec(),
            pyramid: scene.dataset.pyramid().clone(),
            reader,
        }
    }

    pub fn stored(reader: Arc<Image>) -> Self {
        let pyramid = reader.pyramid().clone();
        Self {
            reader,
            masks: Vec::new(),
            pyramid,
        }
    }

    pub fn image(&self) -> &Image {
        &self.reader
    }

    pub fn image_shared(&self) -> Arc<Image> {
        Arc::clone(&self.reader)
    }

    pub fn pyramid(&self) -> &Pyramid {
        &self.pyramid
    }

    pub fn name(&self) -> &str {
        self.reader.name()
    }

    pub fn format(&self) -> Option<Format> {
        self.reader.format()
    }

    pub fn labels(&self) -> Result<Labels, grana_image::Error> {
        self.reader.labels()
    }

    fn stored_channels(&self) -> usize {
        self.reader.pyramid().channel_count()
    }

    pub fn read_region(
        &self,
        time: u32,
        level: usize,
        channel: usize,
        ranges: &[Range<u32>; 3],
        shift: [f32; 3],
    ) -> Result<VoxelData, ChannelsError> {
        let stored = self.stored_channels();
        if channel < stored {
            return self
                .reader
                .read_region(time, level, channel, ranges, shift)
                .map_err(ChannelsError::Image);
        }
        let mask_channel =
            self.masks
                .get(channel - stored)
                .ok_or_else(|| ChannelsError::Derived {
                    detail: format!(
                        "the dataset holds {stored} stored channels and no channel {channel}"
                    ),
                })?;
        let data = self
            .reader
            .read_region(time, level, mask_channel.source(), ranges, shift)
            .map_err(ChannelsError::Image)?;
        let info =
            self.pyramid
                .levels()
                .get(level)
                .copied()
                .ok_or_else(|| ChannelsError::Derived {
                    detail: format!("the derived channel has no resolution level {level}"),
                })?;
        let mask = mask_channel
            .mask(time, info)
            .map_err(|detail| ChannelsError::Derived { detail })?;
        Ok(masked(&data, ranges, &mask))
    }

    pub fn read_brick(
        &self,
        time: u32,
        level: usize,
        coord: BrickCoord,
        shift: [f32; 3],
    ) -> Result<BrickData, ChannelsError> {
        let data = self
            .reader
            .read_brick(time, level, coord, shift)
            .map_err(ChannelsError::Image)?;
        if self.masks.is_empty() {
            return Ok(data);
        }
        let info =
            self.pyramid
                .levels()
                .get(level)
                .copied()
                .ok_or_else(|| ChannelsError::Derived {
                    detail: format!("the derived channel has no resolution level {level}"),
                })?;
        let grid = BrickGrid::new(info.extent());
        let mut channels = data.channels().to_vec();
        for mask_channel in &self.masks {
            let mask = mask_channel
                .mask(time, info)
                .map_err(|detail| ChannelsError::Derived { detail })?;
            channels.push(masked_brick(
                &data.channels()[mask_channel.source()],
                &mask,
                grid,
                coord,
            ));
        }
        Ok(BrickData::new(channels))
    }
}

fn masked_brick(data: &VoxelData, mask: &Mask, grid: BrickGrid, coord: BrickCoord) -> VoxelData {
    match data {
        VoxelData::U8(values) => VoxelData::U8(mask_cube(values, mask, grid, coord)),
        VoxelData::U16(values) => VoxelData::U16(mask_cube(values, mask, grid, coord)),
        VoxelData::F32(values) => VoxelData::F32(mask_cube(values, mask, grid, coord)),
    }
}

fn mask_cube<T: Copy + Default>(
    values: &[T],
    mask: &Mask,
    grid: BrickGrid,
    coord: BrickCoord,
) -> Vec<T> {
    assert_eq!(
        values.len(),
        (PADDED_SIDE as usize).pow(3),
        "the masked cube holds every padded voxel"
    );
    let mut masked = values.to_vec();
    let mut index = 0;
    for z in 0..PADDED_SIDE {
        for y in 0..PADDED_SIDE {
            for x in 0..PADDED_SIDE {
                if !mask.contains_cell(grid.padded_cell(coord, [x, y, z])) {
                    masked[index] = T::default();
                }
                index += 1;
            }
        }
    }
    masked
}

pub fn masked(data: &VoxelData, ranges: &[Range<u32>; 3], mask: &Mask) -> VoxelData {
    let width = (ranges[0].end - ranges[0].start).max(1);
    let height = (ranges[1].end - ranges[1].start).max(1);
    let depth = (ranges[2].end - ranges[2].start).max(1);
    match data {
        VoxelData::U8(values) => {
            VoxelData::U8(mask_values(values, ranges, width, height, depth, mask))
        }
        VoxelData::U16(values) => {
            VoxelData::U16(mask_values(values, ranges, width, height, depth, mask))
        }
        VoxelData::F32(values) => {
            VoxelData::F32(mask_values(values, ranges, width, height, depth, mask))
        }
    }
}

fn mask_values<T: Copy + Default>(
    values: &[T],
    ranges: &[Range<u32>; 3],
    width: u32,
    height: u32,
    depth: u32,
    mask: &Mask,
) -> Vec<T> {
    assert_eq!(
        values.len(),
        (width * height * depth) as usize,
        "the masked channel covers every voxel of the request"
    );
    let mut masked = values.to_vec();
    let mut index = 0;
    for z in 0..depth {
        for y in 0..height {
            for x in 0..width {
                let cell = [
                    ranges[0].start + x,
                    ranges[1].start + y,
                    ranges[2].start + z,
                ];
                if !mask.contains_cell(cell) {
                    masked[index] = T::default();
                }
                index += 1;
            }
        }
    }
    masked
}
