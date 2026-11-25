use burn::prelude::*;
use burn::tensor::activation::{relu, sigmoid, tanh};
use burn::tensor::module::interpolate;
use burn::tensor::ops::{InterpolateMode, InterpolateOptions};

use crate::Result;
use crate::model::checkpoint::{ChannelNorm, Checkpoint, Conv3d};

const EMBEDDING: usize = 256;
const FEATURES: [usize; 4] = [256, 128, 64, 32];
const OUTPUT_CHANNELS: usize = 5;
const NORMALIZATION_EPSILON: f64 = 1.0e-5;
const UPSAMPLE_BUDGET: u64 = 1_500_000_000;

struct InstanceNorm;

impl InstanceNorm {
    fn forward(input: Tensor<5>) -> Tensor<5> {
        let mean = input.clone().mean_dims(&[2, 3, 4]);
        let centered = input - mean;
        let variance = centered.clone().mul(centered.clone()).mean_dims(&[2, 3, 4]);
        centered.div(variance.add_scalar(NORMALIZATION_EPSILON).sqrt())
    }
}

struct Strip {
    first: Conv3d,
    norm: ChannelNorm,
    second: Conv3d,
}

impl Strip {
    fn load(
        checkpoint: &Checkpoint,
        prefix: &str,
        channels: usize,
        device: &Device,
    ) -> Result<Self> {
        let hidden = channels / 4;
        Ok(Self {
            first: Conv3d::load(
                checkpoint,
                &format!("{prefix}.conv1.weight"),
                &format!("{prefix}.conv1.bias"),
                [hidden, channels, 1, 1, 1],
                device,
            )?,
            norm: ChannelNorm::load(checkpoint, &format!("{prefix}.bn1"), device)?,
            second: Conv3d::load(
                checkpoint,
                &format!("{prefix}.conv2.weight"),
                &format!("{prefix}.conv2.bias"),
                [channels, hidden, 1, 1, 1],
                device,
            )?,
        })
    }

    fn forward(&self, input: Tensor<5>) -> Tensor<5> {
        let [_, _, depth, _, _] = input.dims();
        if depth == 1 {
            return input;
        }
        let pooled = input.clone().mean_dim(2);
        let gate = sigmoid(
            self.second.forward(
                relu(
                    self.norm
                        .forward(self.first.forward(pooled, [0, 0, 0], [1, 1, 1])),
                ),
                [0, 0, 0],
                [1, 1, 1],
            ),
        );
        let [batch, channels, depth, height, width] = input.dims();
        input.clone() * gate.expand([batch, channels, depth, height, width]) + input
    }
}

struct ConvBlock {
    first: Conv3d,
    second: Conv3d,
    strip: Strip,
}

impl ConvBlock {
    fn load(
        checkpoint: &Checkpoint,
        prefix: &str,
        input: usize,
        output: usize,
        device: &Device,
    ) -> Result<Self> {
        Ok(Self {
            first: Conv3d::load(
                checkpoint,
                &format!("{prefix}.block.0.block.1.weight"),
                &format!("{prefix}.block.0.block.1.bias"),
                [output, input, 3, 3, 3],
                device,
            )?,
            second: Conv3d::load(
                checkpoint,
                &format!("{prefix}.block.0.block.4.weight"),
                &format!("{prefix}.block.0.block.4.bias"),
                [output, output, 3, 3, 3],
                device,
            )?,
            strip: Strip::load(checkpoint, &format!("{prefix}.block.1"), output, device)?,
        })
    }

    fn forward(&self, input: Tensor<5>) -> Tensor<5> {
        let x = relu(
            self.first
                .forward(InstanceNorm::forward(input), [1, 1, 1], [1, 1, 1]),
        );
        let x = relu(
            self.second
                .forward(InstanceNorm::forward(x), [1, 1, 1], [1, 1, 1]),
        );
        self.strip.forward(x)
    }
}

struct DeconvBlock {
    upsample: Conv3d,
    conv: Conv3d,
    norm: ChannelNorm,
    strip: Strip,
}

impl DeconvBlock {
    fn load(
        checkpoint: &Checkpoint,
        prefix: &str,
        input: usize,
        output: usize,
        device: &Device,
    ) -> Result<Self> {
        Ok(Self {
            upsample: Conv3d::load(
                checkpoint,
                &format!("{prefix}.block.0.conv.weight"),
                &format!("{prefix}.block.0.conv.bias"),
                [output, input, 1, 1, 1],
                device,
            )?,
            conv: Conv3d::load(
                checkpoint,
                &format!("{prefix}.block.1.weight"),
                &format!("{prefix}.block.1.bias"),
                [output, output, 1, 3, 3],
                device,
            )?,
            norm: ChannelNorm::load(checkpoint, &format!("{prefix}.block.2"), device)?,
            strip: Strip::load(checkpoint, &format!("{prefix}.block.4"), output, device)?,
        })
    }

    fn forward(&self, input: Tensor<5>) -> Tensor<5> {
        let [_, channels, depth, height, width] = input.dims();
        let per_slice = channels as u64 * 4 * (height as u64 * 2) * (width as u64 * 2);
        let chunk = (UPSAMPLE_BUDGET / per_slice.max(1)).max(1) as usize;
        let mut parts = Vec::with_capacity(depth.div_ceil(chunk));
        let mut start = 0;
        while start < depth {
            let length = chunk.min(depth - start);
            let part = upsample2x(input.clone().narrow(2, start, length));
            parts.push(self.upsample.forward(part, [0, 0, 0], [1, 1, 1]));
            start += length;
        }
        let x = if parts.len() == 1 {
            parts.pop().expect("one upsampled part")
        } else {
            Tensor::cat(parts, 2)
        };
        let x = self.conv.forward(x, [0, 1, 1], [1, 1, 1]);
        let x = relu(self.norm.forward(x));
        self.strip.forward(x)
    }
}

struct Head {
    first_left: Conv3d,
    first_right: Conv3d,
    second: Conv3d,
    strip: Strip,
}

impl Head {
    fn load(checkpoint: &Checkpoint, device: &Device) -> Result<Self> {
        let first = Conv3d::load(
            checkpoint,
            "decoder_head.block.0.block.1.weight",
            "decoder_head.block.0.block.1.bias",
            [FEATURES[3], FEATURES[3] * 2, 3, 3, 3],
            device,
        )?;
        let first_right = Conv3d {
            weight: first.weight.clone().narrow(1, FEATURES[3], FEATURES[3]),
            bias: first.bias.clone() * 0.0,
        };
        let first_left = Conv3d {
            weight: first.weight.narrow(1, 0, FEATURES[3]),
            bias: first.bias,
        };
        Ok(Self {
            first_left,
            first_right,
            second: Conv3d::load(
                checkpoint,
                "decoder_head.block.0.block.4.weight",
                "decoder_head.block.0.block.4.bias",
                [FEATURES[3], FEATURES[3], 3, 3, 3],
                device,
            )?,
            strip: Strip::load(checkpoint, "decoder_head.block.1", FEATURES[3], device)?,
        })
    }

    fn forward(&self, left: Tensor<5>, right: Tensor<5>) -> Tensor<5> {
        let left = self
            .first_left
            .forward(InstanceNorm::forward(left), [1, 1, 1], [1, 1, 1]);
        let right = self
            .first_right
            .forward(InstanceNorm::forward(right), [1, 1, 1], [1, 1, 1]);
        let x = relu(left + right);
        let x = relu(
            self.second
                .forward(InstanceNorm::forward(x), [1, 1, 1], [1, 1, 1]),
        );
        self.strip.forward(x)
    }
}

struct Sampler {
    conv: Conv3d,
}

impl Sampler {
    fn load(
        checkpoint: &Checkpoint,
        prefix: &str,
        input: usize,
        output: usize,
        device: &Device,
    ) -> Result<Self> {
        Ok(Self {
            conv: Conv3d::load(
                checkpoint,
                &format!("{prefix}.conv.weight"),
                &format!("{prefix}.conv.bias"),
                [output, input, 1, 1, 1],
                device,
            )?,
        })
    }

    fn forward(&self, input: Tensor<5>) -> Tensor<5> {
        self.conv.forward(upsample2x(input), [0, 0, 0], [1, 1, 1])
    }
}

pub(crate) struct Decoder {
    base: ConvBlock,
    deconv: [DeconvBlock; 4],
    blocks: Vec<ConvBlock>,
    samplers: Vec<Sampler>,
    output: DeconvBlock,
    head: Head,
    readout: Conv3d,
}

impl Decoder {
    pub(crate) fn load(checkpoint: &Checkpoint, device: &Device) -> Result<Self> {
        let mut deconv = Vec::with_capacity(4);
        for index in 0..4 {
            let input = if index == 0 {
                EMBEDDING
            } else {
                FEATURES[index - 1]
            };
            deconv.push(DeconvBlock::load(
                checkpoint,
                &format!("deconv{}", index + 1),
                input,
                FEATURES[index],
                device,
            )?);
        }
        let mut blocks = Vec::with_capacity(3);
        let mut samplers = Vec::with_capacity(3);
        for index in 0..3 {
            blocks.push(ConvBlock::load(
                checkpoint,
                &format!("decoder.blocks.{index}"),
                FEATURES[index + 1] * 2,
                FEATURES[index + 1],
                device,
            )?);
            samplers.push(Sampler::load(
                checkpoint,
                &format!("decoder.samplers.{index}"),
                FEATURES[index],
                FEATURES[index + 1],
                device,
            )?);
        }
        let deconv: [DeconvBlock; 4] = deconv
            .try_into()
            .map_err(|_| crate::error::Error::model("the decoder needs four deconvolutions"))?;
        Ok(Self {
            base: ConvBlock::load(checkpoint, "base", EMBEDDING, FEATURES[0], device)?,
            deconv,
            blocks,
            samplers,
            output: DeconvBlock::load(checkpoint, "deconv_out", FEATURES[3], FEATURES[3], device)?,
            head: Head::load(checkpoint, device)?,
            readout: Conv3d::load(
                checkpoint,
                "out_conv.weight",
                "out_conv.bias",
                [OUTPUT_CHANNELS, FEATURES[3], 1, 1, 1],
                device,
            )?,
        })
    }

    pub(crate) fn forward(&self, features: Tensor<5>) -> Tensor<5> {
        activate(self.forward_stages(features))
    }

    fn forward_stages(&self, features: Tensor<5>) -> Tensor<5> {
        let first = self.deconv[0].forward(features.clone());
        let second = self.deconv[1].forward(first.clone());
        let third = self.deconv[2].forward(second.clone());
        let shallow = self.deconv[3].forward(third.clone());
        let mut x = self.base.forward(features);
        for (index, (block, sampler)) in self.blocks.iter().zip(&self.samplers).enumerate() {
            x = sampler.forward(x);
            let skip = match index {
                0 => &first,
                1 => &second,
                _ => &third,
            };
            let combined = Tensor::cat(
                vec![x, center_channels(skip.clone(), FEATURES[index + 1])],
                1,
            );
            x = block.forward(combined);
        }
        let x = self.output.forward(x);
        let x = self.head.forward(x, shallow);
        self.readout.forward(x, [0, 0, 0], [1, 1, 1])
    }
}

fn center_channels(input: Tensor<5>, channels: usize) -> Tensor<5> {
    let current = input.dims()[1];
    if current == channels {
        return input;
    }
    input.narrow(1, (current - channels) / 2, channels)
}

fn upsample2x(input: Tensor<5>) -> Tensor<5> {
    let [batch, channels, depth, height, width] = input.dims();
    let flat = input.reshape([batch * channels * depth, 1, height, width]);
    let flat = interpolate(
        flat,
        InterpolateOptions::new(InterpolateMode::Bilinear)
            .with_align_corners(false)
            .with_scale_factor([2.0, 2.0]),
    );
    flat.reshape([batch, channels, depth, height * 2, width * 2])
}

fn activate(logits: Tensor<5>) -> Tensor<5> {
    let foreground = sigmoid(logits.clone().narrow(1, 0, 1));
    let distances = tanh(logits.clone().narrow(1, 1, 3));
    let boundary = sigmoid(logits.narrow(1, 4, 1));
    Tensor::cat(vec![foreground, distances, boundary], 1)
}
