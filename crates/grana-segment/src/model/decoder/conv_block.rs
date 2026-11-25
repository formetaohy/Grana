use burn::prelude::*;
use burn::tensor::activation::relu;

use crate::Result;
use crate::model::checkpoint::{Checkpoint, Conv3d};
use crate::model::decoder::deconv::{gate_of, joined};
use crate::model::decoder::parts::{Parts, split};
use crate::model::decoder::span::{Source, Span, Stripe};
use crate::model::decoder::strip::Strip;
use crate::model::decoder::sums::{ColumnSums, DepthSums, Statistics};

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

    pub(crate) fn forward(&self, source: Source<'_>, stripe: Stripe, depth: usize) -> Tensor<5> {
        joined(self.gated(source, stripe, depth))
    }

    pub(crate) fn split(&self, source: Source<'_>, stripe: Stripe, depth: usize) -> Parts {
        split(stripe, self.gated(source, stripe, depth))
    }

    fn gated(&self, source: Source<'_>, stripe: Stripe, depth: usize) -> Vec<Tensor<5>> {
        let samples = depth * stripe.side() * stripe.side();

        let mut inputs: Option<ColumnSums> = None;
        for span in stripe.spans() {
            accumulate(&mut inputs, &source(span.context(0)));
        }
        let first_stats = inputs.expect("a stripe holds one span").statistics(samples);

        let mut raised_sums: Option<ColumnSums> = None;
        let mut held = Vec::new();
        for span in stripe.spans() {
            let raised = relu(self.first.forward(
                first_stats.normalise(source(span.context(1))),
                [1, 1, 1],
                [1, 1, 1],
            ));
            accumulate(&mut raised_sums, &span.crop(raised.clone(), 1));
            held.push(stripe.whole_width().then_some(raised));
        }
        let second_stats = raised_sums
            .expect("a stripe holds one span")
            .statistics(samples);

        let mut parts = Vec::new();
        for (index, span) in stripe.spans().into_iter().enumerate() {
            let activation = self.release(source, span, &first_stats, &second_stats, &held, index);
            let mut sums = DepthSums::of(&activation);
            sums.add(&activation);
            parts.push(gate_of(&self.strip, &sums, depth).apply(activation));
        }
        parts
    }

    fn release(
        &self,
        source: Source<'_>,
        span: Span,
        first_stats: &Statistics,
        second_stats: &Statistics,
        held: &[Option<Tensor<5>>],
        index: usize,
    ) -> Tensor<5> {
        let front = span.front(1);
        let raised = match held.get(index).and_then(Option::as_ref) {
            Some(raised) => raised.clone(),
            None => {
                let whole = relu(self.first.forward(
                    first_stats.normalise(source(span.context(2))),
                    [1, 1, 1],
                    [1, 1, 1],
                ));
                whole.narrow(
                    4,
                    span.front(2) - front,
                    span.extent() + front + span.back(1),
                )
            }
        };
        let widened = self
            .second
            .forward(second_stats.normalise(raised), [1, 1, 1], [1, 1, 1]);
        relu(span.crop(widened, 1))
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
