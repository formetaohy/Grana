use burn::prelude::*;
use burn::tensor::activation::relu;

use crate::Result;
use crate::model::checkpoint::{Checkpoint, Conv3d};
use crate::model::decoder::deconv::gate_of;
use crate::model::decoder::parts::{Parts, split};
use crate::model::decoder::span::{Source, Stripe};
use crate::model::decoder::strip::Strip;
use crate::model::decoder::sums::{ColumnSums, DepthSums};

pub(crate) struct ConvBlock {
    first: Conv3d,
    second: Conv3d,
    strip: Strip,
}

impl ConvBlock {
    pub(crate) fn load(
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

    pub(crate) fn split(&self, source: Source<'_>, stripe: Stripe) -> Parts {
        split(stripe, self.gated(source, stripe))
    }

    fn gated(&self, source: Source<'_>, stripe: Stripe) -> Vec<Tensor<5>> {
        let spans = stripe.spans();
        let mut inputs: Option<ColumnSums> = None;
        for &span in &spans {
            accumulate(&mut inputs, &source(span.context(0)));
        }
        let first_stats = inputs.expect("a stripe holds one span").statistics();

        let mut raised_sums: Option<ColumnSums> = None;
        let mut raised_parts = Vec::with_capacity(spans.len());
        for &span in &spans {
            let window = source(span.context(1));
            let raised = relu(self.first.forward(first_stats.normalise(window), [0, 1, 1]));
            accumulate(&mut raised_sums, &span.crop(raised.clone(), 1));
            raised_parts.push((span, raised));
        }
        let second_stats = raised_sums.expect("a stripe holds one span").statistics();

        let mut parts = Vec::with_capacity(raised_parts.len());
        for (span, raised) in raised_parts {
            let widened = self
                .second
                .forward(second_stats.normalise(raised), [0, 1, 1]);
            let activation = relu(span.crop(widened, 1));
            let mut sums = DepthSums::of(&activation);
            sums.add(&activation);
            parts.push(gate_of(&self.strip, &sums).apply(activation));
        }
        parts
    }
}

fn accumulate(holder: &mut Option<ColumnSums>, tensor: &Tensor<5>) {
    let mut sums = ColumnSums::of(tensor);
    sums.add(tensor);
    *holder = Some(match holder.take() {
        None => sums,
        Some(held) => held.join(sums),
    });
}
