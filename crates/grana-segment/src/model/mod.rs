mod checkpoint;
mod conv;
pub(crate) mod decoder;
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
        let path = path.to_path_buf();
        let (encoder, decoder, weights) = device.memory_persistent_allocations(
            (path, device.clone()),
            |(path, device)| -> Result<_> {
                let checkpoint = checkpoint::Checkpoint::open(&path)?;
                let weights = checkpoint.bytes();
                let encoder = encoder::Encoder::load(&checkpoint, &device)?;
                let decoder = decoder::Decoder::load(&checkpoint, &device)?;
                Ok((encoder, decoder, weights))
            },
        )?;
        Ok(Self {
            encoder,
            decoder,
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
        self.encoder.forward(input, queries)
    }

    pub(crate) fn decode(
        &self,
        features: &dyn Fn(std::ops::Range<usize>) -> Result<Tensor<5>>,
        depth: usize,
        splits: usize,
        emit: &mut dyn FnMut(usize, Tensor<5>) -> Result<()>,
    ) -> Result<()> {
        self.decoder.decode(features, depth, splits, emit)
    }
}
