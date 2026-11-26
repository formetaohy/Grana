use std::path::Path;
use std::sync::Arc;

use burn::prelude::*;
use burn::tensor::TensorData;
use burn::tensor::module::interpolate;
use burn::tensor::ops::{InterpolateMode, InterpolateOptions};

use crate::Result;
use crate::cache;
use crate::error::Error;
use crate::model::Model;
use crate::normalize;
use crate::plan::{Plan, SIDE};
use crate::postprocess::{self, Prediction};
use crate::settings::Settings;
use crate::volume::Volume;

const TILING_THRESHOLD: u32 = 768;
const TILE: u32 = 512;
const HALO: u32 = 128;
const SLICE_TILE: u32 = 4;
const SLICE_HALO: u32 = 2;
const CONTEXT: [u32; 3] = [HALO, HALO, SLICE_HALO];
const PREDICTION_BYTES_PER_VOXEL: u64 = (CHANNELS * size_of::<f32>()) as u64;
const PREDICTION_BUDGET: u64 = 2 << 30;
const CHANNELS: usize = 4;
const MODEL_CHANNELS: usize = 5;
const MEAN: [f32; 3] = [0.485, 0.456, 0.406];
const STANDARD_DEVIATION: [f32; 3] = [0.229, 0.224, 0.225];

pub struct Segmenter {
    model: Arc<Model>,
    settings: Settings,
}

impl Segmenter {
    pub fn open(path: &Path, settings: Settings) -> Result<Self> {
        Ok(Self {
            model: cache::acquire(path)?,
            settings,
        })
    }

    pub fn settings(&self) -> Settings {
        self.settings
    }

    pub fn context(&self) -> [u32; 3] {
        CONTEXT
    }

    pub fn voxel_budget(&self) -> u64 {
        PREDICTION_BUDGET / PREDICTION_BYTES_PER_VOXEL
    }

    pub fn plan(&self, depth: u32, spare: u64) -> Result<Plan> {
        Plan::of(self.model.weights(), tile_depth(depth) as usize, spare)
    }

    pub fn graphics_memory(&self) -> Option<u64> {
        self.model
            .device()
            .memory_pool_usage()
            .map(|usage| usage.bytes_reserved)
    }

    pub fn segment(
        &self,
        planes: &[Vec<f32>],
        width: u32,
        height: u32,
        plan: Plan,
        progress: &mut dyn FnMut(usize),
    ) -> Result<Volume> {
        let prediction = self.predict(planes, width, height, plan, progress)?;
        let labels = postprocess::labels(&prediction, self.settings);
        Ok(Volume::new(width, height, planes.len() as u32, labels))
    }

    fn predict(
        &self,
        planes: &[Vec<f32>],
        width: u32,
        height: u32,
        plan: Plan,
        progress: &mut dyn FnMut(usize),
    ) -> Result<Prediction> {
        let depth = planes.len() as u32;
        if width == 0 || height == 0 || depth == 0 {
            return Err(Error::input(
                "a segmented volume needs a positive width, height and depth",
            ));
        }
        let area = (width as usize)
            .checked_mul(height as usize)
            .ok_or_else(|| Error::input("the plane dimensions overflow the address space"))?;
        for (index, plane) in planes.iter().enumerate() {
            if plane.len() != area {
                return Err(Error::input(format!(
                    "plane {index} must contain {area} values, found {}",
                    plane.len()
                )));
            }
            if plane.iter().any(|value| !value.is_finite()) {
                return Err(Error::input(
                    "a segmented plane contains a non-finite value",
                ));
            }
        }
        let voxels = (area as u64)
            .checked_mul(u64::from(depth))
            .ok_or_else(|| Error::input("the volume dimensions overflow the address space"))?;
        if voxels > self.voxel_budget() {
            return Err(Error::input(
                "the segmentation region exceeds the prediction budget; narrow the region",
            ));
        }
        let voxels = usize::try_from(voxels)
            .map_err(|_| Error::input("the volume dimensions overflow the address space"))?;
        let mut channels = std::array::from_fn(|_| vec![0f32; voxels]);
        let tiled = width > TILING_THRESHOLD || height > TILING_THRESHOLD;
        let x_blocks = if tiled {
            blocking(width, TILE, HALO)
        } else {
            vec![whole(width)]
        };
        let y_blocks = if tiled {
            blocking(height, TILE, HALO)
        } else {
            vec![whole(height)]
        };
        let mut done = 0usize;
        for z in depth_blocks(depth) {
            for y in &y_blocks {
                for x in &x_blocks {
                    self.infer(
                        planes,
                        [width, height, depth],
                        [*x, *y, z],
                        plan,
                        &mut channels,
                    )?;
                }
            }
            done += z.inner as usize;
            progress(done);
        }
        Ok(Prediction {
            width: width as usize,
            height: height as usize,
            depth: depth as usize,
            channels,
        })
    }

    fn infer(
        &self,
        planes: &[Vec<f32>],
        shape: [u32; 3],
        blocks: [Block; 3],
        plan: Plan,
        channels: &mut [Vec<f32>; CHANNELS],
    ) -> Result<()> {
        let [x, y, z] = blocks;
        let [width, height, depth] = shape;
        let tile_width = x.extent + 2 * x.halo;
        let tile_height = y.extent + 2 * y.halo;
        let tile_depth = z.extent + 2 * z.halo;
        let tile_area = tile_width as usize * tile_height as usize;
        let (resized_height, resized_width) =
            resized_shape(tile_height as usize, tile_width as usize);
        let device = self.model.device().clone();
        let mean =
            Tensor::<4>::from_data(TensorData::new(MEAN.to_vec(), vec![1, 3, 1, 1]), &device);
        let deviation = Tensor::<4>::from_data(
            TensorData::new(STANDARD_DEVIATION.to_vec(), vec![1, 3, 1, 1]),
            &device,
        );
        let mut features = Vec::with_capacity(tile_depth as usize);
        for local_z in 0..tile_depth {
            let source_z = reflect(
                z.start as isize + local_z as isize - z.halo as isize,
                depth as usize,
            );
            let plane = &planes[source_z];
            let mut values = Vec::with_capacity(tile_area);
            for local_y in 0..tile_height {
                let source_y = reflect(
                    y.start as isize + local_y as isize - y.halo as isize,
                    height as usize,
                );
                let row = source_y * width as usize;
                for local_x in 0..tile_width {
                    let source_x = reflect(
                        x.start as isize + local_x as isize - x.halo as isize,
                        width as usize,
                    );
                    values.push(plane[row + source_x]);
                }
            }
            let values = normalize::plane(&values);
            let tensor = Tensor::<4>::from_data(
                TensorData::new(
                    values,
                    vec![1, 1, tile_height as usize, tile_width as usize],
                ),
                &device,
            );
            let resized = interpolate(
                tensor,
                InterpolateOptions::new(InterpolateMode::Bilinear)
                    .with_align_corners(false)
                    .with_output_size([resized_height, resized_width]),
            );
            let rgb = Tensor::cat(vec![resized; 3], 1);
            let normalised = (rgb - mean.clone()) / deviation.clone();
            let padded = normalised.pad(
                [
                    (0, 0),
                    (0, 0),
                    (0, SIDE - resized_height),
                    (0, SIDE - resized_width),
                ],
                burn::tensor::ops::PadMode::Constant(0.0),
            );
            features.push(self.model.encode(padded, plan.queries())?);
        }
        let features = Tensor::cat(features, 0);

        let halo = z.halo as usize;
        let inner = z.inner as usize;
        self.model
            .decode(features, plan.splits(), &mut |index, slice| {
                if index < halo || index >= halo + inner {
                    return Ok(());
                }
                let local_z = index - halo;
                let resized = interpolate(
                    slice
                        .reshape([1, MODEL_CHANNELS, SIDE, SIDE])
                        .narrow(2, 0, resized_height)
                        .narrow(3, 0, resized_width),
                    InterpolateOptions::new(InterpolateMode::Bilinear)
                        .with_align_corners(false)
                        .with_output_size([tile_height as usize, tile_width as usize]),
                );
                let values = resized.into_data().try_to_vec::<f32>().map_err(|error| {
                    Error::inference(format!("the model output cannot be read: {error}"))
                })?;
                for channel in 0..CHANNELS {
                    let source = &values[channel * tile_area..(channel + 1) * tile_area];
                    for local_y in 0..y.inner as usize {
                        let source_row =
                            (local_y + y.halo as usize) * tile_width as usize + x.halo as usize;
                        let target_y = y.start as usize + local_y;
                        let target_z = z.start as usize + local_z;
                        let target = x.start as usize
                            + width as usize * (target_y + height as usize * target_z);
                        channels[channel][target..target + x.inner as usize]
                            .copy_from_slice(&source[source_row..source_row + x.inner as usize]);
                    }
                }
                Ok(())
            })
    }
}

#[derive(Clone, Copy)]
struct Block {
    start: u32,
    inner: u32,
    extent: u32,
    halo: u32,
}

fn whole(length: u32) -> Block {
    Block {
        start: 0,
        inner: length,
        extent: length,
        halo: 0,
    }
}

fn tile_depth(depth: u32) -> u32 {
    if depth <= SLICE_TILE {
        depth
    } else {
        SLICE_TILE + 2 * SLICE_HALO
    }
}

fn depth_blocks(depth: u32) -> Vec<Block> {
    if depth <= SLICE_TILE {
        return vec![whole(depth)];
    }
    blocking(depth, SLICE_TILE, SLICE_HALO)
}

fn blocking(length: u32, tile: u32, halo: u32) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut start = 0;
    while start < length {
        let inner = tile.min(length - start);
        blocks.push(Block {
            start,
            inner,
            extent: tile,
            halo,
        });
        start += tile;
    }
    blocks
}

fn reflect(index: isize, length: usize) -> usize {
    if length == 1 {
        return 0;
    }
    let length = length as isize;
    let period = 2 * (length - 1);
    let mut value = index.rem_euclid(period);
    if value >= length {
        value = period - value;
    }
    value as usize
}

fn resized_shape(height: usize, width: usize) -> (usize, usize) {
    let scale = SIDE as f64 / height.max(width) as f64;
    (
        (height as f64 * scale + 0.5) as usize,
        (width as f64 * scale + 0.5) as usize,
    )
}
