use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::ops::Range;
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

const TILING_THRESHOLD: u32 = SIDE as u32;
const TILE: u32 = 768;
const HALO: u32 = 128;
const SLAB: u32 = 8;
const CONTEXT: [u32; 3] = [HALO, HALO, SLAB];
const PREDICTION_BYTES_PER_VOXEL: u64 = (CHANNELS * size_of::<f32>()) as u64;
const PREDICTION_BUDGET: u64 = 2 << 30;
const CHANNELS: usize = 4;
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
        Plan::of(
            self.model.weights(),
            depth as usize,
            spare,
            self.model.peaks(),
        )
    }

    #[doc(hidden)]
    pub fn forced_plan(&self, depth: u32, splits: usize) -> Plan {
        Plan::forced(
            splits,
            Plan::estimate(
                self.model.weights(),
                depth as usize,
                splits,
                self.model.peaks(),
            ),
        )
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
        let baseline = self
            .model
            .device()
            .memory_pool_report()
            .map(|pools| pools.iter().map(|pool| pool.pages_peak).collect::<Vec<_>>());
        let prediction = self.predict(planes, width, height, plan, progress)?;
        if let Some(baseline) = baseline {
            self.account(planes.len() as u32, plan, &baseline);
        }
        let labels = postprocess::labels(&prediction, self.settings);
        Ok(Volume::new(width, height, planes.len() as u32, labels))
    }

    fn account(&self, depth: u32, plan: Plan, baseline: &[u64]) {
        let Some(pools) = self.model.device().memory_pool_report() else {
            return;
        };
        let mut measured = 0u64;
        for (position, pool) in pools.iter().enumerate() {
            let before = baseline.get(position).copied().unwrap_or(0);
            measured += pool
                .page_size
                .saturating_mul(pool.pages_peak.saturating_sub(before));
        }
        self.model
            .peaks()
            .record(depth as usize, plan.splits(), measured);
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
        let tiles = (x_blocks.len() * y_blocks.len()).max(1);
        let emitted = Cell::new(0usize);
        for y in &y_blocks {
            for x in &x_blocks {
                self.infer(
                    planes,
                    [width, height, depth],
                    [*x, *y],
                    plan,
                    &mut channels,
                    &mut || {
                        emitted.set(emitted.get() + 1);
                        progress((emitted.get() / tiles).min(depth as usize));
                    },
                )?;
            }
        }
        progress(depth as usize);
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
        blocks: [Block; 2],
        plan: Plan,
        channels: &mut [Vec<f32>; CHANNELS],
        slice_done: &mut dyn FnMut(),
    ) -> Result<()> {
        let [x, y] = blocks;
        let [width, height, depth] = shape;
        let tile_width = x.extent + 2 * x.halo;
        let tile_height = y.extent + 2 * y.halo;
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

        let cache = RefCell::new(FeatureCache {
            encoded: VecDeque::new(),
            frontier: 0,
        });
        let encode = |slice: usize| -> Result<Tensor<4>> {
            let plane = &planes[slice];
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
            self.model.encode(padded, plan.queries())
        };

        let features = |range: Range<usize>| -> Result<Tensor<5>> {
            let mut cache = cache.borrow_mut();
            while cache.frontier < range.end {
                let feature = encode(cache.frontier)?;
                cache.encoded.push_back(feature);
                cache.frontier += 1;
            }
            cache.evict(range.start);
            cache.window(range)
        };

        let copy = Copy {
            width: width as usize,
            height: height as usize,
            tile_width: tile_width as usize,
            tile_height: tile_height as usize,
            tile_area,
            x_start: x.start as usize,
            y_start: y.start as usize,
            x_inner: x.inner as usize,
            y_inner: y.inner as usize,
            x_halo: x.halo as usize,
            y_halo: y.halo as usize,
            resized_height,
            resized_width,
        };
        self.model.decode(
            &features,
            depth as usize,
            plan.splits(),
            &mut |start: usize, slices: Tensor<5>| {
                let count = slices.dims()[2];
                let flattened = slices
                    .reshape([1, CHANNELS, count, SIDE, SIDE])
                    .permute([0, 2, 1, 3, 4])
                    .reshape([count, CHANNELS, SIDE, SIDE])
                    .narrow(2, 0, resized_height)
                    .narrow(3, 0, resized_width);
                let resized = interpolate(
                    flattened,
                    InterpolateOptions::new(InterpolateMode::Bilinear)
                        .with_align_corners(false)
                        .with_output_size([tile_height as usize, tile_width as usize]),
                );
                let values = resized.into_data().try_to_vec::<f32>().map_err(|error| {
                    Error::inference(format!("the model output cannot be read: {error}"))
                })?;
                let plane = CHANNELS * tile_height as usize * tile_width as usize;
                for index in 0..count {
                    copy.planes(
                        channels,
                        start + index,
                        &values[index * plane..(index + 1) * plane],
                    );
                    slice_done();
                }
                Ok(())
            },
        )
    }
}

struct FeatureCache {
    encoded: VecDeque<Tensor<4>>,
    frontier: usize,
}

impl FeatureCache {
    fn evict(&mut self, keep_from: usize) {
        while self.frontier - self.encoded.len() < keep_from && !self.encoded.is_empty() {
            self.encoded.pop_front();
        }
    }

    fn window(&self, range: Range<usize>) -> Result<Tensor<5>> {
        let start = self.frontier - self.encoded.len();
        let mut pieces = Vec::with_capacity(range.end - range.start);
        for slice in range {
            let index = slice.checked_sub(start).ok_or_else(|| {
                Error::inference("the feature window lost a slice it still needs")
            })?;
            let feature = self.encoded.get(index).ok_or_else(|| {
                Error::inference("the feature window lost a slice it still needs")
            })?;
            let [batch, feature_channels, feature_height, feature_width] = feature.dims();
            pieces.push(feature.clone().reshape([
                batch,
                feature_channels,
                1,
                feature_height,
                feature_width,
            ]));
        }
        Ok(Tensor::cat(pieces, 2))
    }
}

struct Copy {
    width: usize,
    height: usize,
    tile_width: usize,
    tile_height: usize,
    tile_area: usize,
    x_start: usize,
    y_start: usize,
    x_inner: usize,
    y_inner: usize,
    x_halo: usize,
    y_halo: usize,
    resized_height: usize,
    resized_width: usize,
}

impl Copy {
    fn planes(&self, channels: &mut [Vec<f32>; CHANNELS], index: usize, values: &[f32]) {
        let _ = self.tile_height;
        let _ = self.resized_height;
        let _ = self.resized_width;
        for channel in 0..CHANNELS {
            let source = &values[channel * self.tile_area..(channel + 1) * self.tile_area];
            for local_y in 0..self.y_inner {
                let source_row = (local_y + self.y_halo) * self.tile_width + self.x_halo;
                let target_y = self.y_start + local_y;
                let target = self.x_start + self.width * (target_y + self.height * index);
                channels[channel][target..target + self.x_inner]
                    .copy_from_slice(&source[source_row..source_row + self.x_inner]);
            }
        }
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
