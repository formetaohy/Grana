use std::path::Path;

use burn::prelude::*;
use burn::tensor::TensorData;
use burn::tensor::activation::{gelu, softmax};
use burn::tensor::module::{conv_transpose2d, conv2d, layer_norm};
use burn::tensor::ops::{ConvOptions, ConvTransposeOptions};

use crate::Result;
use crate::dynamics::Flow;
use crate::error::Error;
use crate::weights::{self, BlockNames, Checkpoint, NeckNames};

const NORMALIZATION_EPSILON: f64 = 1.0e-6;
const ROW_EQUATION: &str = "bhwc,hkc->bhwk";
const COLUMN_EQUATION: &str = "bhwc,wkc->bhwk";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModelInfo {
    side: u32,
}

impl ModelInfo {
    pub fn side(&self) -> u32 {
        self.side
    }
}

#[derive(Clone, Copy)]
struct Geometry {
    grid: usize,
    heads: usize,
    head_dimension: usize,
}

impl Geometry {
    fn tokens(self) -> usize {
        self.grid * self.grid
    }

    fn scale(self) -> f32 {
        (self.head_dimension as f64).sqrt().recip() as f32
    }
}

struct Linear {
    weight: Tensor<2>,
    bias: Tensor<2>,
}

impl Linear {
    fn forward(&self, input: Tensor<2>) -> Tensor<2> {
        input.matmul(self.weight.clone()) + self.bias.clone()
    }

    fn load(checkpoint: &Checkpoint, weight: &str, bias: &str, device: &Device) -> Result<Self> {
        let matrix = matrix(checkpoint, weight, device)?;
        let width = matrix.dims()[1];
        let values = checkpoint.values(bias)?;
        Ok(Self {
            weight: matrix,
            bias: Tensor::from_data(TensorData::new(values, vec![1, width]), device),
        })
    }
}

struct Norm {
    weight: Tensor<1>,
    bias: Tensor<1>,
}

impl Norm {
    fn forward<const D: usize>(&self, input: Tensor<D>) -> Tensor<D> {
        layer_norm(
            input,
            self.weight.clone(),
            Some(self.bias.clone()),
            NORMALIZATION_EPSILON,
        )
    }

    fn load(checkpoint: &Checkpoint, weight: &str, bias: &str, device: &Device) -> Result<Self> {
        Ok(Self {
            weight: tensor(checkpoint, weight, device)?,
            bias: tensor(checkpoint, bias, device)?,
        })
    }
}

struct Attention {
    query_key_value: Linear,
    projection: Linear,
    row: Tensor<3>,
    column: Tensor<3>,
}

impl Attention {
    fn forward(&self, input: Tensor<4>, geometry: Geometry) -> Tensor<4> {
        let [batch, height, width, dimension] = input.dims();
        let tokens = geometry.tokens();
        let heads = geometry.heads;
        let head_dimension = geometry.head_dimension;
        let grid = geometry.grid;
        let folded = self
            .query_key_value
            .forward(input.reshape([tokens, dimension]))
            .reshape([tokens, 3, heads, head_dimension])
            .permute([1, 2, 0, 3]);
        let query = folded
            .clone()
            .narrow(0, 0, 1)
            .reshape([heads, tokens, head_dimension]);
        let key = folded
            .clone()
            .narrow(0, 1, 1)
            .reshape([heads, tokens, head_dimension]);
        let value = folded
            .narrow(0, 2, 1)
            .reshape([heads, tokens, head_dimension]);
        let table = query.clone().reshape([heads, grid, grid, head_dimension]);
        let query = query.mul_scalar(geometry.scale());
        let scores = query.matmul(key.transpose());
        let row: Tensor<4> = Tensor::einsum(
            ROW_EQUATION,
            [table.clone().into(), self.row.clone().into()],
        );
        let column: Tensor<4> =
            Tensor::einsum(COLUMN_EQUATION, [table.into(), self.column.clone().into()]);
        let scores = scores.reshape([heads, grid, grid, grid, grid])
            + row.reshape([heads, grid, grid, grid, 1])
            + column.reshape([heads, grid, grid, 1, grid]);
        let weights = softmax(scores.reshape([heads, tokens, tokens]), 2);
        let attended = weights
            .matmul(value)
            .reshape([batch, heads, grid, grid, head_dimension])
            .permute([0, 2, 3, 1, 4])
            .reshape([tokens, dimension]);
        self.projection
            .forward(attended)
            .reshape([batch, height, width, dimension])
    }

    fn load(
        checkpoint: &Checkpoint,
        names: &BlockNames,
        geometry: Geometry,
        device: &Device,
    ) -> Result<Self> {
        Ok(Self {
            query_key_value: Linear::load(checkpoint, &names.qkv_weight, &names.qkv_bias, device)?,
            projection: Linear::load(checkpoint, &names.proj_weight, &names.proj_bias, device)?,
            row: relative_positions(
                checkpoint,
                &names.row,
                geometry.grid,
                geometry.head_dimension,
                device,
            )?,
            column: relative_positions(
                checkpoint,
                &names.column,
                geometry.grid,
                geometry.head_dimension,
                device,
            )?,
        })
    }
}

struct Perceptron {
    first: Linear,
    second: Linear,
}

impl Perceptron {
    fn forward(&self, input: Tensor<4>, geometry: Geometry) -> Tensor<4> {
        let [batch, height, width, dimension] = input.dims();
        let tokens = geometry.tokens();
        let hidden = gelu(self.first.forward(input.reshape([tokens, dimension])));
        self.second
            .forward(hidden)
            .reshape([batch, height, width, dimension])
    }

    fn load(checkpoint: &Checkpoint, names: &BlockNames, device: &Device) -> Result<Self> {
        Ok(Self {
            first: Linear::load(
                checkpoint,
                &names.mlp_one_weight,
                &names.mlp_one_bias,
                device,
            )?,
            second: Linear::load(
                checkpoint,
                &names.mlp_two_weight,
                &names.mlp_two_bias,
                device,
            )?,
        })
    }
}

struct Block {
    norm_one: Norm,
    attention: Attention,
    norm_two: Norm,
    perceptron: Perceptron,
}

impl Block {
    fn forward(&self, input: Tensor<4>, geometry: Geometry) -> Tensor<4> {
        let attended = input.clone()
            + self
                .attention
                .forward(self.norm_one.forward(input), geometry);
        attended.clone()
            + self
                .perceptron
                .forward(self.norm_two.forward(attended), geometry)
    }

    fn load(
        checkpoint: &Checkpoint,
        names: &BlockNames,
        geometry: Geometry,
        device: &Device,
    ) -> Result<Self> {
        Ok(Self {
            norm_one: Norm::load(
                checkpoint,
                &names.norm_one_weight,
                &names.norm_one_bias,
                device,
            )?,
            attention: Attention::load(checkpoint, names, geometry, device)?,
            norm_two: Norm::load(
                checkpoint,
                &names.norm_two_weight,
                &names.norm_two_bias,
                device,
            )?,
            perceptron: Perceptron::load(checkpoint, names, device)?,
        })
    }
}

struct ChannelNorm {
    weight: Tensor<4>,
    bias: Tensor<4>,
}

impl ChannelNorm {
    fn load(
        checkpoint: &Checkpoint,
        weight: &str,
        bias: &str,
        channels: usize,
        device: &Device,
    ) -> Result<Self> {
        let weight: Tensor<1> = tensor(checkpoint, weight, device)?;
        let bias: Tensor<1> = tensor(checkpoint, bias, device)?;
        Ok(Self {
            weight: weight.reshape([1, channels, 1, 1]),
            bias: bias.reshape([1, channels, 1, 1]),
        })
    }

    fn forward(&self, input: Tensor<4>) -> Tensor<4> {
        let mean = input.clone().mean_dim(1);
        let centered = input - mean;
        let variance = centered.clone().mul(centered.clone()).mean_dim(1);
        let normalized = centered.div(variance.add_scalar(NORMALIZATION_EPSILON).sqrt());
        normalized * self.weight.clone() + self.bias.clone()
    }
}

struct Neck {
    first_weight: Tensor<4>,
    first_norm: ChannelNorm,
    second_weight: Tensor<4>,
    second_norm: ChannelNorm,
}

impl Neck {
    fn load(
        checkpoint: &Checkpoint,
        names: &NeckNames,
        channels: usize,
        device: &Device,
    ) -> Result<Self> {
        Ok(Self {
            first_weight: tensor(checkpoint, &names.first_weight, device)?,
            first_norm: ChannelNorm::load(
                checkpoint,
                &names.first_norm_weight,
                &names.first_norm_bias,
                channels,
                device,
            )?,
            second_weight: tensor(checkpoint, &names.second_weight, device)?,
            second_norm: ChannelNorm::load(
                checkpoint,
                &names.second_norm_weight,
                &names.second_norm_bias,
                channels,
                device,
            )?,
        })
    }

    fn forward(&self, input: Tensor<4>) -> Tensor<4> {
        let x = conv2d(
            input,
            self.first_weight.clone(),
            None,
            ConvOptions::new([1, 1], [0, 0], [1, 1], 1),
        );
        let x = self.first_norm.forward(x);
        let x = conv2d(
            x,
            self.second_weight.clone(),
            None,
            ConvOptions::new([1, 1], [1, 1], [1, 1], 1),
        );
        self.second_norm.forward(x)
    }
}

struct Head {
    weight: Tensor<4>,
    bias: Tensor<1>,
    upsample: Tensor<4>,
}

impl Head {
    fn load(checkpoint: &Checkpoint, device: &Device) -> Result<Self> {
        Ok(Self {
            weight: tensor(checkpoint, weights::OUT_WEIGHT, device)?,
            bias: tensor(checkpoint, weights::OUT_BIAS, device)?,
            upsample: tensor(checkpoint, weights::UPSAMPLE, device)?,
        })
    }

    fn forward(&self, input: Tensor<4>, patch: usize) -> Tensor<4> {
        let x = conv2d(
            input,
            self.weight.clone(),
            Some(self.bias.clone()),
            ConvOptions::new([1, 1], [0, 0], [1, 1], 1),
        );
        conv_transpose2d(
            x,
            self.upsample.clone(),
            None,
            ConvTransposeOptions::new([patch, patch], [0, 0], [0, 0], [1, 1], 1),
        )
    }
}

pub struct Model {
    device: Device,
    info: ModelInfo,
    geometry: Geometry,
    patch: usize,
    outputs: usize,
    embedding_weight: Tensor<4>,
    embedding_bias: Tensor<1>,
    position: Tensor<4>,
    blocks: Vec<Block>,
    neck: Neck,
    head: Head,
}

impl Model {
    pub fn open(path: &Path) -> Result<Self> {
        let checkpoint = Checkpoint::open(path)?;
        let layout = Layout::resolve(&checkpoint)?;
        let device = crate::device::open()?;
        let info = ModelInfo {
            side: u32::try_from(layout.grid * layout.patch)
                .map_err(|_| Error::model("the model tile exceeds the pixel range"))?,
        };
        Self::build(&checkpoint, &layout, device, info)
    }

    fn build(
        checkpoint: &Checkpoint,
        layout: &Layout,
        device: Device,
        info: ModelInfo,
    ) -> Result<Self> {
        let geometry = Geometry {
            grid: layout.grid,
            heads: layout.heads,
            head_dimension: layout.head_dimension,
        };
        let embedding_weight = tensor(checkpoint, weights::PATCH_WEIGHT, &device)?;
        let embedding_bias = tensor(checkpoint, weights::PATCH_BIAS, &device)?;
        let position = tensor(checkpoint, weights::POSITION, &device)?;
        let mut blocks = Vec::with_capacity(layout.blocks);
        for index in 0..layout.blocks {
            let names = checkpoint.block_names(index);
            blocks.push(Block::load(checkpoint, &names, geometry, &device)?);
        }
        let neck = Neck::load(
            checkpoint,
            &checkpoint.neck_names(),
            layout.channels,
            &device,
        )?;
        let head = Head::load(checkpoint, &device)?;
        Ok(Self {
            device,
            info,
            geometry,
            patch: layout.patch,
            outputs: layout.outputs,
            embedding_weight,
            embedding_bias,
            position,
            blocks,
            neck,
            head,
        })
    }

    pub fn info(&self) -> &ModelInfo {
        &self.info
    }

    pub fn infer(&self, tile: &[f32]) -> Result<Flow> {
        let side = self.info.side;
        let area = side as usize * side as usize;
        let length = area * 3;
        if tile.len() != length {
            return Err(Error::input(format!(
                "the model tile must contain {length} values, found {}",
                tile.len()
            )));
        }
        let input = Tensor::<4>::from_data(
            TensorData::new(tile.to_vec(), vec![1, 3, side as usize, side as usize]),
            &self.device,
        );
        let output = self.forward(input);
        let values = output.into_data().try_to_vec::<f32>().map_err(|error| {
            Error::inference(format!("the model output cannot be read: {error}"))
        })?;
        let expected = area * self.outputs;
        if values.len() != expected {
            return Err(Error::inference(format!(
                "the model must predict {expected} values, found {}",
                values.len()
            )));
        }
        let mut values = values;
        let cellprob = values.split_off(area * 2);
        let dx = values.split_off(area);
        let dy = values;
        if dy
            .iter()
            .chain(dx.iter())
            .chain(cellprob.iter())
            .any(|value| !value.is_finite())
        {
            return Err(Error::inference(
                "the model produced non-finite flow values",
            ));
        }
        Ok(Flow::from_parts(side, dy, dx, cellprob))
    }

    fn forward(&self, image: Tensor<4>) -> Tensor<4> {
        let embedding = conv2d(
            image,
            self.embedding_weight.clone(),
            Some(self.embedding_bias.clone()),
            ConvOptions::new([self.patch, self.patch], [0, 0], [1, 1], 1),
        );
        let mut x = embedding.permute([0, 2, 3, 1]) + self.position.clone();
        for block in &self.blocks {
            x = block.forward(x, self.geometry);
        }
        self.head
            .forward(self.neck.forward(x.permute([0, 3, 1, 2])), self.patch)
    }
}

struct Layout {
    patch: usize,
    grid: usize,
    heads: usize,
    head_dimension: usize,
    channels: usize,
    outputs: usize,
    blocks: usize,
}

impl Layout {
    fn resolve(checkpoint: &Checkpoint) -> Result<Self> {
        let patch = checkpoint.shape(weights::PATCH_WEIGHT)?;
        expect_rank(&patch, 4, weights::PATCH_WEIGHT)?;
        if patch[1] != 3 || patch[2] != patch[3] {
            return Err(Error::model(format!(
                "the patch embedding must map three channels through a square kernel, found {patch:?}"
            )));
        }
        let dimension = patch[0];
        let kernel = patch[2];
        expect(checkpoint, weights::PATCH_BIAS, &[dimension])?;
        let position = checkpoint.shape(weights::POSITION)?;
        if position.len() != 4
            || position[0] != 1
            || position[1] != position[2]
            || position[3] != dimension
        {
            return Err(Error::model(format!(
                "the position embedding must match the token grid, found {position:?}"
            )));
        }
        let grid = position[1];
        let blocks = checkpoint.block_count();
        if blocks == 0 {
            return Err(Error::model("the weights hold no transformer block"));
        }
        for index in 0..blocks {
            let names = checkpoint.block_names(index);
            expect(checkpoint, &names.norm_one_weight, &[dimension])?;
            expect(checkpoint, &names.norm_one_bias, &[dimension])?;
            expect(checkpoint, &names.norm_two_weight, &[dimension])?;
            expect(checkpoint, &names.norm_two_bias, &[dimension])?;
            expect(checkpoint, &names.qkv_weight, &[dimension * 3, dimension])?;
            expect(checkpoint, &names.qkv_bias, &[dimension * 3])?;
            expect(checkpoint, &names.proj_weight, &[dimension, dimension])?;
            expect(checkpoint, &names.proj_bias, &[dimension])?;
            expect(
                checkpoint,
                &names.mlp_one_weight,
                &[dimension * 4, dimension],
            )?;
            expect(checkpoint, &names.mlp_one_bias, &[dimension * 4])?;
            expect(
                checkpoint,
                &names.mlp_two_weight,
                &[dimension, dimension * 4],
            )?;
            expect(checkpoint, &names.mlp_two_bias, &[dimension])?;
        }
        let first = checkpoint.block_names(0);
        let table = checkpoint.shape(&first.row)?;
        if table.len() != 2 || table[1] == 0 {
            return Err(Error::model(format!(
                "the relative row table must have two dimensions, found {table:?}"
            )));
        }
        let head_dimension = table[1];
        if dimension % head_dimension != 0 {
            return Err(Error::model(format!(
                "the head width {head_dimension} does not divide the model width {dimension}"
            )));
        }
        let channel_table = checkpoint.shape(&first.column)?;
        if channel_table.len() != 2 || channel_table[1] != head_dimension {
            return Err(Error::model(format!(
                "the relative column table must match the row table, found {channel_table:?}"
            )));
        }
        let neck = checkpoint.neck_names();
        let channels = checkpoint.shape(&neck.first_weight)?[0];
        expect(checkpoint, &neck.first_weight, &[channels, dimension, 1, 1])?;
        expect(checkpoint, &neck.second_weight, &[channels, channels, 3, 3])?;
        expect(checkpoint, &neck.first_norm_weight, &[channels])?;
        expect(checkpoint, &neck.first_norm_bias, &[channels])?;
        expect(checkpoint, &neck.second_norm_weight, &[channels])?;
        expect(checkpoint, &neck.second_norm_bias, &[channels])?;
        let upsample = checkpoint.shape(weights::UPSAMPLE)?;
        expect_rank(&upsample, 4, weights::UPSAMPLE)?;
        if upsample[2] != kernel || upsample[3] != kernel {
            return Err(Error::model(format!(
                "the readout must stride the patch kernel, found {upsample:?}"
            )));
        }
        let outputs = upsample[1];
        expect(
            checkpoint,
            weights::OUT_WEIGHT,
            &[outputs * kernel * kernel, channels, 1, 1],
        )?;
        expect(checkpoint, weights::OUT_BIAS, &[outputs * kernel * kernel])?;
        Ok(Self {
            patch: kernel,
            grid,
            heads: dimension / head_dimension,
            head_dimension,
            channels,
            outputs,
            blocks,
        })
    }
}

fn tensor<const D: usize>(
    checkpoint: &Checkpoint,
    name: &str,
    device: &Device,
) -> Result<Tensor<D>> {
    let shape = checkpoint.shape(name)?;
    let values = checkpoint.values(name)?;
    Ok(Tensor::<D>::from_data(
        TensorData::new(values, shape),
        device,
    ))
}

fn matrix(checkpoint: &Checkpoint, name: &str, device: &Device) -> Result<Tensor<2>> {
    let shape = checkpoint.shape(name)?;
    if shape.len() != 2 {
        return Err(Error::model(format!(
            "the weights `{name}` must hold a matrix, found {shape:?}"
        )));
    }
    let (rows, columns) = (shape[0], shape[1]);
    let values = checkpoint.values(name)?;
    let mut transposed = vec![0f32; values.len()];
    for row in 0..rows {
        for column in 0..columns {
            transposed[column * rows + row] = values[row * columns + column];
        }
    }
    Ok(Tensor::from_data(
        TensorData::new(transposed, vec![columns, rows]),
        device,
    ))
}

fn relative_positions(
    checkpoint: &Checkpoint,
    name: &str,
    grid: usize,
    head_dimension: usize,
    device: &Device,
) -> Result<Tensor<3>> {
    let shape = checkpoint.shape(name)?;
    if shape.len() != 2 || shape[1] != head_dimension {
        return Err(Error::model(format!(
            "the relative position table `{name}` must hold {head_dimension} values per row, found {shape:?}"
        )));
    }
    let rows = shape[0];
    if rows == 0 || grid == 0 {
        return Err(Error::model("the relative position table is empty"));
    }
    let values = checkpoint.values(name)?;
    let span = 2 * grid - 1;
    let resampled = resample(&values, rows, span, head_dimension);
    let mut gathered = vec![0f32; grid * grid * head_dimension];
    for query in 0..grid {
        for key in 0..grid {
            let source = (query + grid - 1 - key) * head_dimension;
            let target = (query * grid + key) * head_dimension;
            gathered[target..target + head_dimension]
                .copy_from_slice(&resampled[source..source + head_dimension]);
        }
    }
    Ok(Tensor::from_data(
        TensorData::new(gathered, vec![grid, grid, head_dimension]),
        device,
    ))
}

fn resample(values: &[f32], rows: usize, span: usize, width: usize) -> Vec<f32> {
    let scale = rows as f64 / span as f64;
    let mut resampled = vec![0f32; span * width];
    for target in 0..span {
        let position = (((target as f64 + 0.5) * scale - 0.5).max(0.0)).min((rows - 1) as f64);
        let lower = position.floor() as usize;
        let upper = (lower + 1).min(rows - 1);
        let weight = (position - lower as f64) as f32;
        for column in 0..width {
            let start = values[lower * width + column];
            let end = values[upper * width + column];
            resampled[target * width + column] = start + (end - start) * weight;
        }
    }
    resampled
}

fn expect(checkpoint: &Checkpoint, name: &str, expected: &[usize]) -> Result<()> {
    let shape = checkpoint.shape(name)?;
    if shape != expected {
        return Err(Error::model(format!(
            "the weights `{name}` must have the shape {expected:?}, found {shape:?}"
        )));
    }
    Ok(())
}

fn expect_rank(shape: &[usize], rank: usize, name: &str) -> Result<()> {
    if shape.len() != rank {
        return Err(Error::model(format!(
            "the weights `{name}` must have rank {rank}, found {shape:?}"
        )));
    }
    Ok(())
}
