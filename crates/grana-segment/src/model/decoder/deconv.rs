use std::ops::Range;

use burn::prelude::*;
use burn::tensor::activation::relu;
use burn::tensor::module::interpolate;
use burn::tensor::ops::{InterpolateMode, InterpolateOptions};

use crate::Result;
use crate::model::checkpoint::{ChannelNorm, Checkpoint, Conv3d};
use crate::model::decoder::parts::{Parts, split};
use crate::model::decoder::span::{Source, Span, Stripe, parent, raised};
use crate::model::decoder::strip::{Gate, Strip, no_gate};
use crate::model::decoder::sums::DepthSums;

pub(crate) struct DeconvBlock {
    upsample: Conv3d,
    conv: Conv3d,
    norm: ChannelNorm,
    strip: Strip,
}

impl DeconvBlock {
    pub(crate) fn load(
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

    pub(crate) fn split(&self, source: Source<'_>, stripe: Stripe) -> Parts {
        let tensors = stripe
            .spans()
            .into_iter()
            .map(|span| self.span(source, span, stripe.side()));
        split(stripe, tensors)
    }

    pub(crate) fn span(&self, source: Source<'_>, span: Span, side: usize) -> Tensor<5> {
        let activation = self.activation(source, span.context(0), side);
        let mut sums = DepthSums::of(&activation);
        sums.add(&activation);
        gate_of(&self.strip, &sums).apply(activation)
    }

    fn activation(&self, source: Source<'_>, target: Range<usize>, side: usize) -> Tensor<5> {
        let parents = parent(&target, side / 2);
        let input = source(parents.clone());
        let lifted = self.upsample.forward(input, [0, 0, 0], [1, 1, 1]);
        let widened = upsample2x(lifted);
        let activated = relu(
            self.norm
                .forward(self.conv.forward(widened, [0, 1, 1], [1, 1, 1])),
        );
        activated.narrow(4, raised(&target, &parents), target.len())
    }
}

pub(crate) struct Sampler {
    conv: Conv3d,
}

impl Sampler {
    pub(crate) fn load(
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

    pub(crate) fn forward(&self, input: Tensor<5>) -> Tensor<5> {
        upsample2x(self.conv.forward(input, [0, 0, 0], [1, 1, 1]))
    }
}

pub(crate) fn gate_of(strip: &Strip, sums: &DepthSums) -> Gate {
    if sums.depth() <= 1 {
        return no_gate();
    }
    strip.gate(sums.mean())
}

pub(crate) fn upsample2x(input: Tensor<5>) -> Tensor<5> {
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

pub(crate) fn joined(parts: Vec<Tensor<5>>) -> Tensor<5> {
    if parts.len() == 1 {
        return parts.into_iter().next().expect("one decoded part");
    }
    Tensor::cat(parts, 4)
}
