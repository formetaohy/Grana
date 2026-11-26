mod checkpoint;
mod decoder;
mod encoder;

use std::path::Path;

use burn::prelude::*;

use crate::Result;
use crate::plan::Peaks;

pub(crate) struct Model {
    encoder: encoder::Encoder,
    decoder: decoder::Decoder,
    device: Device,
    weights: u64,
    peaks: Peaks,
}

impl Model {
    pub(crate) fn open(path: &Path) -> Result<Self> {
        let device = crate::device::open()?;
        let checkpoint = checkpoint::Checkpoint::open(path)?;
        let weights = checkpoint.bytes();
        Ok(Self {
            encoder: encoder::Encoder::load(&checkpoint, &device)?,
            decoder: decoder::Decoder::load(&checkpoint, &device)?,
            device,
            weights,
            peaks: Peaks::default(),
        })
    }

    pub(crate) fn device(&self) -> &Device {
        &self.device
    }

    pub(crate) fn weights(&self) -> u64 {
        self.weights
    }

    pub(crate) fn peaks(&self) -> &Peaks {
        &self.peaks
    }

    pub(crate) fn encode(&self, input: Tensor<4>, queries: usize) -> Result<Tensor<4>> {
        let features = self.encoder.forward(input, queries)?;
        self.device.sync().map_err(|error| {
            crate::error::Error::device(format!(
                "the graphics device cannot release the encoder workspace: {error}"
            ))
        })?;
        Ok(features)
    }

    pub(crate) fn decode(
        &self,
        features: Tensor<4>,
        splits: usize,
        emit: &mut dyn FnMut(usize, Tensor<5>) -> Result<()>,
    ) -> Result<()> {
        let [slices, channels, height, width] = features.dims();
        let features = features
            .permute([1, 0, 2, 3])
            .reshape([1, channels, slices, height, width]);
        self.decoder.forward(features, splits, emit)
    }
}
