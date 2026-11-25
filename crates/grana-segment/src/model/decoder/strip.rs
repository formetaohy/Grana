use burn::prelude::*;
use burn::tensor::activation::{relu, sigmoid};

use crate::Result;
use crate::model::checkpoint::{ChannelNorm, Checkpoint, Conv3d};

pub(crate) struct Strip {
    first: Conv3d,
    norm: ChannelNorm,
    second: Conv3d,
}

#[derive(Clone)]
pub(crate) struct Gate(Option<Tensor<5>>);

impl Strip {
    pub(crate) fn load(
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

    pub(crate) fn gate(&self, mean: Tensor<5>, depth: usize) -> Gate {
        if depth == 1 {
            return Gate(None);
        }
        let pooled = self.first.forward(mean, [0, 0, 0], [1, 1, 1]);
        let hidden = relu(self.norm.forward(pooled));
        Gate(Some(sigmoid(self.second.forward(
            hidden,
            [0, 0, 0],
            [1, 1, 1],
        ))))
    }
}

impl Gate {
    pub(crate) fn scale(&self) -> Option<&Tensor<5>> {
        self.0.as_ref()
    }

    pub(crate) fn apply(&self, input: Tensor<5>) -> Tensor<5> {
        let Some(gate) = self.0.as_ref() else {
            return input;
        };
        let [batch, channels, depth, height, width] = input.dims();
        input.clone() * (gate.clone() + 1.0).expand([batch, channels, depth, height, width])
    }
}
