mod checkpoint;
mod decoder;
mod encoder;

use std::path::Path;

use burn::prelude::*;

use crate::Result;

pub(crate) struct Model {
    encoder: encoder::Encoder,
    decoder: decoder::Decoder,
    device: Device,
}

impl Model {
    pub(crate) fn open(path: &Path) -> Result<Self> {
        let device = crate::device::open()?;
        let checkpoint = checkpoint::Checkpoint::open(path)?;
        Ok(Self {
            encoder: encoder::Encoder::load(&checkpoint, &device)?,
            decoder: decoder::Decoder::load(&checkpoint, &device)?,
            device,
        })
    }

    pub(crate) fn device(&self) -> &Device {
        &self.device
    }

    pub(crate) fn encode(&self, input: Tensor<4>) -> Result<Tensor<4>> {
        self.encoder.forward(input)
    }

    pub(crate) fn decode(&self, features: Tensor<4>) -> Result<Tensor<5>> {
        let [slices, channels, height, width] = features.dims();
        let features = features
            .permute([1, 0, 2, 3])
            .reshape([1, channels, slices, height, width]);
        Ok(self.decoder.forward(features))
    }
}
