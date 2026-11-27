use burn::prelude::*;
use burn::tensor::activation::gelu;
use burn::tensor::module::interpolate;
use burn::tensor::ops::{InterpolateMode, InterpolateOptions, MaxPoolOptions};

use crate::Result;
use crate::error::Error;
use crate::model::checkpoint::{Checkpoint, Conv2d, Linear, Norm};

const PREFIX: &str = "encoder.inner.";
const DIMENSION: usize = 96;
const BLOCK_COUNT: usize = 12;
const STAGE_ENDS: [usize; 4] = [0, 2, 9, 11];
const STAGE_DIMS: [usize; 4] = [96, 192, 384, 768];
const STAGE_HEADS: [usize; 4] = [1, 2, 4, 8];
const STAGE_WINDOWS: [usize; 4] = [8, 4, 14, 7];
const Q_POOL_BLOCKS: [usize; 3] = [1, 3, 10];
const GLOBAL_BLOCKS: [usize; 3] = [5, 7, 9];
const WINDOW_SIDE: usize = 8;
const POSITION_SIDE: usize = 7;
const NECK_CHANNELS: usize = 256;
const ATTENTION_TOKENS: usize = 4096;
const PERCEPTRON_TOKENS: usize = 8192;
const SHORTCUT_ROWS: usize = 64;

struct Attention {
    query_key_value: Linear,
    projection: Linear,
    heads: usize,
    head_dimension: usize,
    pool: bool,
}

impl Attention {
    fn load(
        checkpoint: &Checkpoint,
        prefix: &str,
        dim_out: usize,
        heads: usize,
        pool: bool,
        device: &Device,
    ) -> Result<Self> {
        Ok(Self {
            query_key_value: Linear::load(
                checkpoint,
                &format!("{prefix}.attn.qkv.weight"),
                &format!("{prefix}.attn.qkv.bias"),
                device,
            )?,
            projection: Linear::load(
                checkpoint,
                &format!("{prefix}.attn.proj.weight"),
                &format!("{prefix}.attn.proj.bias"),
                device,
            )?,
            heads,
            head_dimension: dim_out / heads,
            pool,
        })
    }

    fn forward(&self, input: Tensor<4>, queries: usize) -> Tensor<4> {
        let [batch, height, width, _] = input.dims();
        let step = (ATTENTION_TOKENS / (height * width)).max(1);
        if batch <= step {
            return self.forward_chunk(input, queries);
        }
        let mut parts = Vec::with_capacity(batch.div_ceil(step));
        let mut start = 0;
        while start < batch {
            let length = step.min(batch - start);
            parts.push(self.forward_chunk(input.clone().narrow(0, start, length), queries));
            start += length;
        }
        Tensor::cat(parts, 0)
    }

    fn forward_chunk(&self, input: Tensor<4>, queries: usize) -> Tensor<4> {
        let [batch, height, width, dimension] = input.dims();
        let tokens = height * width;
        let heads = self.heads;
        let head_dimension = self.head_dimension;
        let folded = self
            .query_key_value
            .forward(input.reshape([batch, tokens, dimension]))
            .reshape([batch, tokens, 3, heads, head_dimension])
            .permute([2, 0, 3, 1, 4]);
        let query = folded
            .clone()
            .narrow(0, 0, 1)
            .reshape([batch, heads, tokens, head_dimension]);
        let key = folded
            .clone()
            .narrow(0, 1, 1)
            .reshape([batch, heads, tokens, head_dimension]);
        let value = folded
            .narrow(0, 2, 1)
            .reshape([batch, heads, tokens, head_dimension]);
        let (query, height, width) = if self.pool {
            let pooled = query
                .reshape([batch, heads, height, width, head_dimension])
                .permute([0, 1, 4, 2, 3])
                .reshape([batch * heads, head_dimension, height, width]);
            let pooled = burn::tensor::module::max_pool2d(pooled, MaxPoolOptions::new([2, 2]));
            let [_, _, pooled_height, pooled_width] = pooled.dims();
            let query = pooled
                .reshape([batch, heads, head_dimension, pooled_height * pooled_width])
                .permute([0, 1, 3, 2])
                .reshape([batch, heads, pooled_height * pooled_width, head_dimension]);
            (query, pooled_height, pooled_width)
        } else {
            (query, height, width)
        };
        let attended = attend(query, key, value, queries);
        let attended =
            attended
                .permute([0, 2, 1, 3])
                .reshape([batch, height * width, heads * head_dimension]);
        self.projection
            .forward(attended)
            .reshape([batch, height, width, heads * head_dimension])
    }
}

fn attend(query: Tensor<4>, key: Tensor<4>, value: Tensor<4>, queries: usize) -> Tensor<4> {
    let tokens = query.dims()[2];
    if tokens <= queries {
        return attention(query, key, value);
    }
    let mut parts = Vec::with_capacity(tokens.div_ceil(queries));
    let mut start = 0;
    while start < tokens {
        let length = queries.min(tokens - start);
        parts.push(attention(
            query.clone().narrow(2, start, length),
            key.clone(),
            value.clone(),
        ));
        start += length;
    }
    Tensor::cat(parts, 2)
}

fn attention(query: Tensor<4>, key: Tensor<4>, value: Tensor<4>) -> Tensor<4> {
    burn::tensor::module::attention(
        query,
        key,
        value,
        None,
        None,
        burn::tensor::ops::AttentionModuleOptions::default(),
    )
}

struct Perceptron {
    first: Linear,
    second: Linear,
}

impl Perceptron {
    fn load(checkpoint: &Checkpoint, prefix: &str, device: &Device) -> Result<Self> {
        Ok(Self {
            first: Linear::load(
                checkpoint,
                &format!("{prefix}.mlp.layers.0.weight"),
                &format!("{prefix}.mlp.layers.0.bias"),
                device,
            )?,
            second: Linear::load(
                checkpoint,
                &format!("{prefix}.mlp.layers.1.weight"),
                &format!("{prefix}.mlp.layers.1.bias"),
                device,
            )?,
        })
    }

    fn forward(&self, input: Tensor<4>) -> Tensor<4> {
        let [batch, height, width, dimension] = input.dims();
        let total = batch * height * width;
        let flat = input.reshape([1, total, dimension]);
        let output = if total <= PERCEPTRON_TOKENS {
            self.forward_chunk(flat)
        } else {
            let mut parts = Vec::with_capacity(total.div_ceil(PERCEPTRON_TOKENS));
            let mut start = 0;
            while start < total {
                let length = PERCEPTRON_TOKENS.min(total - start);
                parts.push(self.forward_chunk(flat.clone().narrow(1, start, length)));
                start += length;
            }
            Tensor::cat(parts, 1)
        };
        output.reshape([batch, height, width, dimension])
    }

    fn forward_chunk(&self, input: Tensor<3>) -> Tensor<3> {
        let hidden = gelu(self.first.forward(input));
        self.second.forward(hidden)
    }
}

struct Block {
    dim: usize,
    dim_out: usize,
    window: usize,
    pool: bool,
    norm_one: Norm,
    attention: Attention,
    norm_two: Norm,
    perceptron: Perceptron,
    projection: Option<Linear>,
}

impl Block {
    fn load(checkpoint: &Checkpoint, prefix: &str, index: usize, device: &Device) -> Result<Self> {
        let stage = stage_of(index);
        let start = if stage == 0 {
            0
        } else {
            STAGE_ENDS[stage - 1] + 1
        };
        let dim = if index == start && stage > 0 {
            STAGE_DIMS[stage - 1]
        } else {
            STAGE_DIMS[stage]
        };
        let dim_out = STAGE_DIMS[stage];
        let heads = STAGE_HEADS[stage];
        let window = if GLOBAL_BLOCKS.contains(&index) {
            0
        } else if index == start && stage > 0 {
            STAGE_WINDOWS[stage - 1]
        } else {
            STAGE_WINDOWS[stage]
        };
        let pool = Q_POOL_BLOCKS.contains(&index);
        let projection = if dim == dim_out {
            None
        } else {
            Some(Linear::load(
                checkpoint,
                &format!("{prefix}.proj.weight"),
                &format!("{prefix}.proj.bias"),
                device,
            )?)
        };
        Ok(Self {
            dim,
            dim_out,
            window,
            pool,
            norm_one: Norm::load(
                checkpoint,
                &format!("{prefix}.norm1.weight"),
                &format!("{prefix}.norm1.bias"),
                device,
            )?,
            attention: Attention::load(checkpoint, prefix, dim_out, heads, pool, device)?,
            norm_two: Norm::load(
                checkpoint,
                &format!("{prefix}.norm2.weight"),
                &format!("{prefix}.norm2.bias"),
                device,
            )?,
            perceptron: Perceptron::load(checkpoint, prefix, device)?,
            projection,
        })
    }

    fn forward(&self, input: Tensor<4>, queries: usize) -> Tensor<4> {
        let [batch, height, width, _] = input.dims();
        let normed = self.norm_one.forward(input.clone());
        let shortcut = if self.dim != self.dim_out {
            let projection = self
                .projection
                .as_ref()
                .expect("a widening block holds a projection");
            if self.pool {
                let mut rows = Vec::with_capacity(height.div_ceil(SHORTCUT_ROWS));
                let mut start = 0;
                while start < height {
                    let length = SHORTCUT_ROWS.min(height - start);
                    let strip = normed.clone().narrow(1, start, length);
                    let projected = projection
                        .forward(strip.reshape([batch, length * width, self.dim]))
                        .reshape([batch, length, width, self.dim_out])
                        .permute([0, 3, 1, 2]);
                    let pooled =
                        burn::tensor::module::max_pool2d(projected, MaxPoolOptions::new([2, 2]));
                    rows.push(pooled.permute([0, 2, 3, 1]));
                    start += length;
                }
                Tensor::cat(rows, 1)
            } else {
                projection
                    .forward(normed.clone().reshape([batch, height * width, self.dim]))
                    .reshape([batch, height, width, self.dim_out])
            }
        } else {
            input
        };
        let (windowed, padded_height, padded_width) = if self.window > 0 {
            partition(normed, self.window)
        } else {
            (normed, height, width)
        };
        let attended = self.attention.forward(windowed, queries);
        let attended = if self.window > 0 {
            let window = if self.pool {
                self.window / 2
            } else {
                self.window
            };
            let padded_height = if self.pool {
                padded_height / 2
            } else {
                padded_height
            };
            let padded_width = if self.pool {
                padded_width / 2
            } else {
                padded_width
            };
            let (target_height, target_width) = if self.pool {
                (height / 2, width / 2)
            } else {
                (height, width)
            };
            unpartition(
                attended,
                window,
                padded_height,
                padded_width,
                target_height,
                target_width,
            )
        } else {
            attended
        };
        let attended = shortcut + attended;
        let refined = self
            .perceptron
            .forward(self.norm_two.forward(attended.clone()));
        attended + refined
    }
}

pub(crate) struct Encoder {
    patch_weight: Tensor<4>,
    patch_bias: Tensor<1>,
    position: Tensor<4>,
    window_position: Tensor<4>,
    blocks: Vec<Block>,
    neck_deep: Conv2d,
    neck_shallow: Conv2d,
}

impl Encoder {
    pub(crate) fn load(checkpoint: &Checkpoint, device: &Device) -> Result<Self> {
        Ok(Self {
            patch_weight: checkpoint.tensor(
                &format!("{PREFIX}trunk.patch_embed.proj.weight"),
                &[DIMENSION, 3, 7, 7],
                device,
            )?,
            patch_bias: checkpoint.vector(
                &format!("{PREFIX}trunk.patch_embed.proj.bias"),
                DIMENSION,
                device,
            )?,
            position: checkpoint.tensor(
                &format!("{PREFIX}trunk.pos_embed"),
                &[1, DIMENSION, POSITION_SIDE, POSITION_SIDE],
                device,
            )?,
            window_position: checkpoint.tensor(
                &format!("{PREFIX}trunk.pos_embed_window"),
                &[1, DIMENSION, WINDOW_SIDE, WINDOW_SIDE],
                device,
            )?,
            blocks: (0..BLOCK_COUNT)
                .map(|index| {
                    Block::load(
                        checkpoint,
                        &format!("{PREFIX}trunk.blocks.{index}"),
                        index,
                        device,
                    )
                })
                .collect::<Result<Vec<_>>>()?,
            neck_deep: Conv2d::load(
                checkpoint,
                &format!("{PREFIX}neck.convs.0.conv.weight"),
                &format!("{PREFIX}neck.convs.0.conv.bias"),
                [NECK_CHANNELS, 768, 1, 1],
                device,
            )?,
            neck_shallow: Conv2d::load(
                checkpoint,
                &format!("{PREFIX}neck.convs.1.conv.weight"),
                &format!("{PREFIX}neck.convs.1.conv.bias"),
                [NECK_CHANNELS, 384, 1, 1],
                device,
            )?,
        })
    }

    pub(crate) fn forward(&self, input: Tensor<4>, queries: usize) -> Result<Tensor<4>> {
        let stages = self.stages(input, queries)?;
        let deep = stages
            .get(3)
            .ok_or_else(|| Error::inference("the encoder holds no deep stage"))?;
        let shallow = stages
            .get(2)
            .ok_or_else(|| Error::inference("the encoder holds no shallow stage"))?;
        let deep = self.neck_deep.forward(deep.clone(), [0, 0], [1, 1]);
        let deep = interpolate(
            deep,
            InterpolateOptions::new(InterpolateMode::Nearest)
                .with_align_corners(false)
                .with_scale_factor([2.0, 2.0]),
        );
        let shallow = self.neck_shallow.forward(shallow.clone(), [0, 0], [1, 1]);
        Ok(shallow + deep)
    }

    fn stages(&self, input: Tensor<4>, queries: usize) -> Result<Vec<Tensor<4>>> {
        let [_, channels, height, width] = input.dims();
        if channels != 3 {
            return Err(Error::inference(format!(
                "the image encoder needs three channels, found {channels}"
            )));
        }
        if height % 32 != 0 || width % 32 != 0 {
            return Err(Error::inference(format!(
                "the image encoder needs multiples of 32 pixels, found {height}x{width}"
            )));
        }
        let mut x = self.prelude(input);
        let mut stages = Vec::with_capacity(4);
        for (index, block) in self.blocks.iter().enumerate() {
            x = block.forward(x, queries);
            if STAGE_ENDS.contains(&index) {
                stages.push(x.clone().permute([0, 3, 1, 2]));
            }
        }
        Ok(stages)
    }

    fn prelude(&self, input: Tensor<4>) -> Tensor<4> {
        let [_, _, height, width] = input.dims();
        let x = burn::tensor::module::conv2d(
            input,
            self.patch_weight.clone(),
            Some(self.patch_bias.clone()),
            burn::tensor::ops::ConvOptions::new([4, 4], [3, 3], [1, 1], 1),
        )
        .permute([0, 2, 3, 1]);
        x + position(
            self.position.clone(),
            self.window_position.clone(),
            height / 4,
            width / 4,
        )
    }
}

fn stage_of(index: usize) -> usize {
    STAGE_ENDS
        .iter()
        .position(|end| index <= *end)
        .expect("every block belongs to a stage")
}

fn position(coarse: Tensor<4>, window: Tensor<4>, height: usize, width: usize) -> Tensor<4> {
    let coarse = interpolate(
        coarse,
        InterpolateOptions::new(InterpolateMode::Bicubic)
            .with_align_corners(false)
            .with_output_size([height, width]),
    );
    let [batch, channels, _, _] = window.dims();
    let tiled = window
        .reshape([batch, channels, 1, WINDOW_SIDE, 1, WINDOW_SIDE])
        .expand([
            batch,
            channels,
            height / WINDOW_SIDE,
            WINDOW_SIDE,
            width / WINDOW_SIDE,
            WINDOW_SIDE,
        ])
        .reshape([batch, channels, height, width]);
    (coarse + tiled).permute([0, 2, 3, 1])
}

fn partition(input: Tensor<4>, window: usize) -> (Tensor<4>, usize, usize) {
    let [batch, height, width, channels] = input.dims();
    let pad_height = (window - height % window) % window;
    let pad_width = (window - width % window) % window;
    let padded = input.pad(
        [(0, 0), (0, pad_height), (0, pad_width), (0, 0)],
        burn::tensor::ops::PadMode::Constant(0.0),
    );
    let padded_height = height + pad_height;
    let padded_width = width + pad_width;
    let rows = padded_height / window;
    let columns = padded_width / window;
    let windows = padded
        .reshape([batch, rows, window, columns, window, channels])
        .permute([0, 1, 3, 2, 4, 5])
        .reshape([batch * rows * columns, window, window, channels]);
    (windows, padded_height, padded_width)
}

fn unpartition(
    windows: Tensor<4>,
    window: usize,
    padded_height: usize,
    padded_width: usize,
    height: usize,
    width: usize,
) -> Tensor<4> {
    let [count, _, _, channels] = windows.dims();
    let rows = padded_height / window;
    let columns = padded_width / window;
    let batch = count / (rows * columns);
    windows
        .reshape([batch, rows, columns, window, window, channels])
        .permute([0, 1, 3, 2, 4, 5])
        .reshape([batch, padded_height, padded_width, channels])
        .narrow(1, 0, height)
        .narrow(2, 0, width)
}
