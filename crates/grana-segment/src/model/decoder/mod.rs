mod conv_block;
mod deconv;
mod head;
mod parts;
mod span;
mod strip;
mod sums;

use std::ops::Range;

use burn::prelude::*;
use burn::tensor::activation::{sigmoid, tanh};

use crate::Result;
use crate::model::checkpoint::{Checkpoint, Conv3d};
use crate::model::decoder::conv_block::ConvBlock;
use crate::model::decoder::deconv::{DeconvBlock, Sampler, joined};
use crate::model::decoder::head::Head;
use crate::model::decoder::parts::Parts;
use crate::model::decoder::span::{Stripe, parent, raised};
use crate::model::decoder::sums::{ColumnSums, DepthSums};

const EMBEDDING: usize = 256;
const FEATURES: [usize; 4] = [256, 128, 64, 32];
const OUTPUT_CHANNELS: usize = 5;
const SPAN_FLOOR: usize = 64;

pub(crate) struct Decoder {
    base: ConvBlock,
    deconv: [DeconvBlock; 4],
    blocks: [ConvBlock; 3],
    samplers: [Sampler; 3],
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
        let blocks: [ConvBlock; 3] = blocks
            .try_into()
            .map_err(|_| crate::error::Error::model("the decoder needs three blocks"))?;
        let samplers: [Sampler; 3] = samplers
            .try_into()
            .map_err(|_| crate::error::Error::model("the decoder needs three samplers"))?;
        Ok(Self {
            base: ConvBlock::load(checkpoint, "base", EMBEDDING, FEATURES[0], device)?,
            deconv,
            blocks,
            samplers,
            output: DeconvBlock::load(checkpoint, "deconv_out", FEATURES[3], FEATURES[3], device)?,
            head: Head::load(checkpoint, FEATURES[3], device)?,
            readout: Conv3d::load(
                checkpoint,
                "out_conv.weight",
                "out_conv.bias",
                [OUTPUT_CHANNELS, FEATURES[3], 1, 1, 1],
                device,
            )?,
        })
    }

    pub(crate) fn forward(
        &self,
        features: Tensor<5>,
        splits: usize,
        emit: &mut dyn FnMut(usize, Tensor<5>) -> Result<()>,
    ) -> Result<()> {
        let [_, _, depth, feature_side, _] = features.dims();
        let features_source = |range: Range<usize>| {
            features
                .clone()
                .narrow(4, range.start, range.end - range.start)
        };
        let base = self
            .base
            .forward(&features_source, Stripe::whole(feature_side), depth);
        let first =
            self.deconv[0].forward(&features_source, stripe(feature_side * 2, splits), depth);
        let first_source = |range: Range<usize>| {
            first
                .clone()
                .narrow(4, range.start, range.end - range.start)
        };
        let second = self.deconv[1].forward(&first_source, stripe(feature_side * 4, splits), depth);
        let second_source = |range: Range<usize>| {
            second
                .clone()
                .narrow(4, range.start, range.end - range.start)
        };
        let third_side = feature_side * 8;
        let third = self.deconv[2].split(&second_source, stripe(third_side, splits), depth);

        let first_block_side = feature_side * 2;
        let first_block_source = |range: Range<usize>| {
            Tensor::cat(
                vec![
                    sample(&self.samplers[0], &base, range.clone(), first_block_side),
                    center_channels(
                        first
                            .clone()
                            .narrow(4, range.start, range.end - range.start),
                        FEATURES[1],
                    ),
                ],
                1,
            )
        };
        let branch =
            self.blocks[0].forward(&first_block_source, Stripe::whole(first_block_side), depth);

        let second_block_side = feature_side * 4;
        let second_block_source = |range: Range<usize>| {
            Tensor::cat(
                vec![
                    sample(&self.samplers[1], &branch, range.clone(), second_block_side),
                    center_channels(
                        second
                            .clone()
                            .narrow(4, range.start, range.end - range.start),
                        FEATURES[2],
                    ),
                ],
                1,
            )
        };
        let branch = self.blocks[1].forward(
            &second_block_source,
            Stripe::whole(second_block_side),
            depth,
        );

        let third_block_source = |range: Range<usize>| {
            Tensor::cat(
                vec![
                    sample(&self.samplers[2], &branch, range.clone(), third_side),
                    center_channels(third.source(&range), FEATURES[3]),
                ],
                1,
            )
        };
        let branch = self.blocks[2].split(&third_block_source, stripe(third_side, splits), depth);

        self.tail(&third, &branch, splits, depth, emit)
    }

    fn tail(
        &self,
        third: &Parts,
        branch: &Parts,
        splits: usize,
        depth: usize,
        emit: &mut dyn FnMut(usize, Tensor<5>) -> Result<()>,
    ) -> Result<()> {
        let side = third.side() * 2;
        let stripe = stripe(side, splits);
        let samples = depth * side * side;
        let shallow_source = |range: Range<usize>| third.source(&range);
        let branch_source = |range: Range<usize>| branch.source(&range);

        let shallow = self.deconv[3].measure(&shallow_source, stripe, depth);
        let shallow_stats = shallow.columns().statistics(samples);
        let left = self.output.measure(&branch_source, stripe, depth);
        let left_stats = left.columns().statistics(samples);

        let mut combined: Vec<Tensor<5>> = Vec::new();
        for span in stripe.spans() {
            let target = span.context(1);
            let left = self
                .output
                .project(&branch_source, target.clone(), side, depth);
            let right = self.deconv[3].project(&shallow_source, target, side, depth);
            combined.push(self.head.combine(left, right, &left_stats, &shallow_stats));
        }
        let mut sums: Option<ColumnSums> = None;
        for (span, part) in stripe.spans().into_iter().zip(&combined) {
            let inner = span.crop(part.clone(), 1);
            let mut part_sums = ColumnSums::of(&inner);
            part_sums.add(&inner);
            sums = Some(match sums {
                None => part_sums,
                Some(held) => held.join(part_sums),
            });
        }
        let combined_stats = sums.expect("a stripe holds one span").statistics(samples);

        let mut parts = Vec::new();
        for (span, part) in stripe.spans().into_iter().zip(&combined) {
            let refined = span.crop(self.head.refine(part.clone(), &combined_stats), 1);
            parts.push(self.finish(refined, depth));
        }
        let activated = activate(joined(parts));
        for index in 0..depth {
            emit(index, activated.clone().narrow(2, index, 1))?;
        }
        Ok(())
    }

    fn finish(&self, refined: Tensor<5>, depth: usize) -> Tensor<5> {
        let mut sums = DepthSums::of(&refined);
        sums.add(&refined);
        let gate = self.head.gate(sums.mean(depth), depth);
        self.readout
            .forward(gate.apply(refined), [0, 0, 0], [1, 1, 1])
    }
}

fn stripe(side: usize, splits: usize) -> Stripe {
    Stripe::new(side, splits.min(side / SPAN_FLOOR).max(1))
}

fn sample(sampler: &Sampler, input: &Tensor<5>, range: Range<usize>, side: usize) -> Tensor<5> {
    let parents = parent(&range, side / 2);
    let sampled = sampler.forward(input.clone().narrow(
        4,
        parents.start,
        parents.end - parents.start,
    ));
    sampled.narrow(4, raised(&range, &parents), range.end - range.start)
}

fn center_channels(input: Tensor<5>, channels: usize) -> Tensor<5> {
    let current = input.dims()[1];
    if current == channels {
        return input;
    }
    input.narrow(1, (current - channels) / 2, channels)
}

fn activate(logits: Tensor<5>) -> Tensor<5> {
    let foreground = sigmoid(logits.clone().narrow(1, 0, 1));
    let distances = tanh(logits.clone().narrow(1, 1, 3));
    let boundary = sigmoid(logits.narrow(1, 4, 1));
    Tensor::cat(vec![foreground, distances, boundary], 1)
}
