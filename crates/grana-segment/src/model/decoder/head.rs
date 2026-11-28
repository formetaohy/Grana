use burn::prelude::*;
use burn::tensor::activation::relu;

use crate::Result;
use crate::model::checkpoint::{Checkpoint, Conv3d};
use crate::model::decoder::REACH;
use crate::model::decoder::strip::{Gate, Strip};
use crate::model::decoder::sums::Statistics;

pub(crate) struct Head {
    first_left: Conv3d,
    first_right: Conv3d,
    second: Conv3d,
    strip: Strip,
}

impl Head {
    pub(crate) fn load(checkpoint: &Checkpoint, channels: usize, device: &Device) -> Result<Self> {
        let first = Conv3d::load(
            checkpoint,
            "decoder_head.block.0.block.1.weight",
            "decoder_head.block.0.block.1.bias",
            [channels, channels * 2, 3, 3, 3],
            device,
        )?;
        let first_right = first.clone().narrow_inputs(channels, channels).silenced();
        let first_left = first.narrow_inputs(0, channels);
        let head = Self {
            first_left,
            first_right,
            second: Conv3d::load(
                checkpoint,
                "decoder_head.block.0.block.4.weight",
                "decoder_head.block.0.block.4.bias",
                [channels, channels, 3, 3, 3],
                device,
            )?,
            strip: Strip::load(checkpoint, "decoder_head.block.1", channels, device)?,
        };
        assert_eq!(
            REACH,
            head.first_left.radius() + head.second.radius(),
            "a span halo spans the chained convolutions of the head"
        );
        Ok(head)
    }

    pub(crate) fn combine(
        &self,
        left: Tensor<5>,
        right: Tensor<5>,
        left_stats: &Statistics,
        right_stats: &Statistics,
    ) -> Tensor<5> {
        let left = self
            .first_left
            .forward(left_stats.normalise(left), [1, 1, 1]);
        let right = self
            .first_right
            .forward(right_stats.normalise(right), [1, 1, 1]);
        relu(left + right)
    }

    pub(crate) fn refine(&self, combined: Tensor<5>, stats: &Statistics) -> Tensor<5> {
        relu(self.second.forward(stats.normalise(combined), [1, 1, 1]))
    }

    pub(crate) fn gate(&self, mean: Tensor<5>) -> Gate {
        self.strip.gate(mean)
    }
}
