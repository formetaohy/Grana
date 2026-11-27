use burn::prelude::*;
use burn::tensor::activation::{relu, sigmoid};

use crate::Result;
use crate::model::checkpoint::{ChannelNorm, Checkpoint, Conv3d};

pub(crate) struct Strip {
    first: Conv3d,
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
        let first = Conv3d::load(
            checkpoint,
            &format!("{prefix}.conv1.weight"),
            &format!("{prefix}.conv1.bias"),
            [hidden, channels, 1, 1, 1],
            device,
        )?;
        let norm = ChannelNorm::load(checkpoint, &format!("{prefix}.bn1"), device)?;
        Ok(Self {
            first: first.normalized(&norm),
            second: Conv3d::load(
                checkpoint,
                &format!("{prefix}.conv2.weight"),
                &format!("{prefix}.conv2.bias"),
                [channels, hidden, 1, 1, 1],
                device,
            )?,
        })
    }

    pub(crate) fn gate(&self, mean: Tensor<5>) -> Gate {
        let pooled = self.first.forward(mean, [0, 0, 0]);
        Gate(Some(sigmoid(self.second.forward(relu(pooled), [0, 0, 0]))))
    }
}

impl Gate {
    pub(crate) fn apply(&self, input: Tensor<5>) -> Tensor<5> {
        let Some(gate) = self.0.as_ref() else {
            return input;
        };
        let [batch, channels, depth, height, width] = input.dims();
        input.clone() * (gate.clone() + 1.0).expand([batch, channels, depth, height, width])
    }
}

pub(crate) fn no_gate() -> Gate {
    Gate(None)
}
