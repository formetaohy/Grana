use std::path::Path;

use burn::prelude::*;
use burn::store::pytorch::PytorchReader;
use burn::tensor::TensorData;

use crate::Result;
use crate::error::Error;

pub(crate) struct Checkpoint {
    reader: PytorchReader,
}

impl Checkpoint {
    pub(crate) fn open(path: &Path) -> Result<Self> {
        let reader = PytorchReader::new(path).map_err(|error| {
            Error::model(format!(
                "the weights `{}` cannot be read: {error}",
                path.display()
            ))
        })?;
        Ok(Self { reader })
    }

    pub(crate) fn shape(&self, name: &str) -> Result<Vec<usize>> {
        let tensor = self.reader.get(name).ok_or_else(|| missing(name))?;
        Ok(tensor.shape().to_vec())
    }

    pub(crate) fn values(&self, name: &str) -> Result<Vec<f32>> {
        let tensor = self.reader.get(name).ok_or_else(|| missing(name))?;
        let bytes = tensor.read().map_err(|error| {
            Error::model(format!("the weights `{name}` cannot be read: {error}"))
        })?;
        let count = tensor.num_elements();
        match bytes.len().checked_div(count) {
            Some(4) => Ok(bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|word| f32::from_le_bytes(*word))
                .collect()),
            Some(2) => Ok(bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|word| half::f16::from_bits(u16::from_le_bytes(*word)).to_f32())
                .collect()),
            _ => Err(Error::model(format!(
                "the weights `{name}` hold {} bytes for {count} values",
                bytes.len()
            ))),
        }
    }

    pub(crate) fn expect(&self, name: &str, expected: &[usize]) -> Result<()> {
        let shape = self.shape(name)?;
        if shape != expected {
            return Err(Error::model(format!(
                "the weights `{name}` must have the shape {expected:?}, found {shape:?}"
            )));
        }
        Ok(())
    }

    pub(crate) fn tensor<const D: usize>(
        &self,
        name: &str,
        shape: &[usize; D],
        device: &Device,
    ) -> Result<Tensor<D>> {
        self.expect(name, shape)?;
        Ok(Tensor::<D>::from_data(
            TensorData::new(self.values(name)?, shape.to_vec()),
            device,
        ))
    }

    pub(crate) fn vector(&self, name: &str, length: usize, device: &Device) -> Result<Tensor<1>> {
        self.expect(name, &[length])?;
        Ok(Tensor::<1>::from_data(
            TensorData::new(self.values(name)?, vec![length]),
            device,
        ))
    }
}

pub(crate) struct Linear {
    weight: Tensor<2>,
    bias: Tensor<1>,
}

impl Linear {
    pub(crate) fn load(
        checkpoint: &Checkpoint,
        weight: &str,
        bias: &str,
        device: &Device,
    ) -> Result<Self> {
        let [rows, columns] = matrix_shape(checkpoint, weight)?;
        Ok(Self {
            weight: transposed(checkpoint, weight, rows, columns, device)?,
            bias: checkpoint.vector(bias, rows, device)?,
        })
    }

    pub(crate) fn forward(&self, input: Tensor<3>) -> Tensor<3> {
        let [batch, tokens, dimension] = input.dims();
        let width = self.bias.dims()[0];
        (input
            .reshape([batch * tokens, dimension])
            .matmul(self.weight.clone())
            + self.bias.clone().reshape([1, width]))
        .reshape([batch, tokens, width])
    }
}

pub(crate) struct Norm {
    weight: Tensor<1>,
    bias: Tensor<1>,
}

impl Norm {
    pub(crate) fn load(
        checkpoint: &Checkpoint,
        weight: &str,
        bias: &str,
        device: &Device,
    ) -> Result<Self> {
        let length = checkpoint.shape(weight)?[0];
        Ok(Self {
            weight: checkpoint.vector(weight, length, device)?,
            bias: checkpoint.vector(bias, length, device)?,
        })
    }

    pub(crate) fn forward<const D: usize>(&self, input: Tensor<D>) -> Tensor<D> {
        burn::tensor::module::layer_norm(
            input,
            self.weight.clone(),
            Some(self.bias.clone()),
            1.0e-6,
        )
    }
}

pub(crate) struct ChannelNorm {
    weight: Tensor<1>,
    bias: Tensor<1>,
    mean: Tensor<1>,
    variance: Tensor<1>,
}

impl ChannelNorm {
    pub(crate) fn load(checkpoint: &Checkpoint, prefix: &str, device: &Device) -> Result<Self> {
        let weight = format!("{prefix}.weight");
        let length = checkpoint.shape(&weight)?[0];
        Ok(Self {
            weight: checkpoint.vector(&weight, length, device)?,
            bias: checkpoint.vector(&format!("{prefix}.bias"), length, device)?,
            mean: checkpoint.vector(&format!("{prefix}.running_mean"), length, device)?,
            variance: checkpoint.vector(&format!("{prefix}.running_var"), length, device)?,
        })
    }

    pub(crate) fn forward<const D: usize>(&self, input: Tensor<D>) -> Tensor<D> {
        burn::tensor::module::batch_norm(
            input,
            self.weight.clone(),
            self.bias.clone(),
            self.mean.clone(),
            self.variance.clone(),
            1.0e-5,
        )
    }
}

pub(crate) struct Conv2d {
    weight: Tensor<4>,
    bias: Tensor<1>,
}

impl Conv2d {
    pub(crate) fn load(
        checkpoint: &Checkpoint,
        weight: &str,
        bias: &str,
        shape: [usize; 4],
        device: &Device,
    ) -> Result<Self> {
        Ok(Self {
            weight: checkpoint.tensor(weight, &shape, device)?,
            bias: checkpoint.vector(bias, shape[0], device)?,
        })
    }

    pub(crate) fn forward(
        &self,
        input: Tensor<4>,
        padding: [usize; 2],
        stride: [usize; 2],
    ) -> Tensor<4> {
        burn::tensor::module::conv2d(
            input,
            self.weight.clone(),
            Some(self.bias.clone()),
            burn::tensor::ops::ConvOptions::new(stride, padding, [1, 1], 1),
        )
    }
}

pub(crate) struct Conv3d {
    pub(crate) weight: Tensor<5>,
    pub(crate) bias: Tensor<1>,
}

impl Conv3d {
    pub(crate) fn load(
        checkpoint: &Checkpoint,
        weight: &str,
        bias: &str,
        shape: [usize; 5],
        device: &Device,
    ) -> Result<Self> {
        Ok(Self {
            weight: checkpoint.tensor(weight, &shape, device)?,
            bias: checkpoint.vector(bias, shape[0], device)?,
        })
    }

    pub(crate) fn forward(
        &self,
        input: Tensor<5>,
        padding: [usize; 3],
        stride: [usize; 3],
    ) -> Tensor<5> {
        burn::tensor::module::conv3d(
            input,
            self.weight.clone(),
            Some(self.bias.clone()),
            burn::tensor::ops::ConvOptions::new(stride, padding, [1, 1, 1], 1),
        )
    }
}

fn matrix_shape(checkpoint: &Checkpoint, name: &str) -> Result<[usize; 2]> {
    let shape = checkpoint.shape(name)?;
    match shape.as_slice() {
        [rows, columns] => Ok([*rows, *columns]),
        _ => Err(Error::model(format!(
            "the weights `{name}` must hold a matrix, found {shape:?}"
        ))),
    }
}

fn transposed(
    checkpoint: &Checkpoint,
    name: &str,
    rows: usize,
    columns: usize,
    device: &Device,
) -> Result<Tensor<2>> {
    let values = checkpoint.values(name)?;
    let mut transposed = vec![0f32; values.len()];
    for row in 0..rows {
        for column in 0..columns {
            transposed[column * rows + row] = values[row * columns + column];
        }
    }
    Ok(Tensor::from_data(
        TensorData::new(transposed, vec![columns, rows]),
        device,
    ))
}

fn missing(name: &str) -> Error {
    Error::model(format!("the weights hold no tensor named `{name}`"))
}
