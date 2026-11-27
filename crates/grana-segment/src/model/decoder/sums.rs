use burn::prelude::*;
const NORMALIZATION_EPSILON: f32 = 1.0e-5;
pub(crate) struct ColumnSums {
    sum: Tensor<5>,
    square: Tensor<5>,
    samples: usize,
}
pub(crate) struct DepthSums {
    sum: Tensor<5>,
    square: Tensor<5>,
    depth: usize,
}
pub(crate) struct Statistics {
    mean: Tensor<5>,
    deviation: Tensor<5>,
}
impl ColumnSums {
    pub(crate) fn of(tensor: &Tensor<5>) -> Self {
        let [_, channels, _, _, width] = tensor.dims();
        let shape = [1, channels, 1, 1, width];
        Self {
            sum: Tensor::zeros(shape, &tensor.device()),
            square: Tensor::zeros(shape, &tensor.device()),
            samples: 0,
        }
    }
    pub(crate) fn add(&mut self, chunk: &Tensor<5>) {
        let [_, _, depth, height, width] = chunk.dims();
        self.samples += depth * height * width;
        self.sum = self.sum.clone() + chunk.clone().sum_dims(&[2, 3]);
        self.square = self.square.clone() + chunk.clone().mul(chunk.clone()).sum_dims(&[2, 3]);
    }
    pub(crate) fn join(self, other: Self) -> Self {
        Self {
            sum: Tensor::cat(vec![self.sum, other.sum], 4),
            square: Tensor::cat(vec![self.square, other.square], 4),
            samples: self.samples + other.samples,
        }
    }
    pub(crate) fn statistics(self) -> Statistics {
        let samples = self.samples;
        Statistics::of(self.sum.sum_dims(&[4]), self.square.sum_dims(&[4]), samples)
    }
}
impl DepthSums {
    pub(crate) fn of(tensor: &Tensor<5>) -> Self {
        let [_, channels, _, height, width] = tensor.dims();
        let shape = [1, channels, 1, height, width];
        Self {
            sum: Tensor::zeros(shape, &tensor.device()),
            square: Tensor::zeros(shape, &tensor.device()),
            depth: 0,
        }
    }
    pub(crate) fn add(&mut self, chunk: &Tensor<5>) {
        self.depth += chunk.dims()[2];
        self.sum = self.sum.clone() + chunk.clone().sum_dims(&[2]);
        self.square = self.square.clone() + chunk.clone().mul(chunk.clone()).sum_dims(&[2]);
    }
    pub(crate) fn depth(&self) -> usize {
        self.depth
    }
    pub(crate) fn mean(&self) -> Tensor<5> {
        self.sum.clone().div_scalar(self.depth.max(1) as f32)
    }
}
impl Statistics {
    fn of(sum: Tensor<5>, square: Tensor<5>, samples: usize) -> Self {
        let samples = samples.max(1) as f32;
        let mean = sum.div_scalar(samples);
        let variance = square.div_scalar(samples) - mean.clone().mul(mean.clone());
        let deviation = variance
            .clamp_min(0.0)
            .add_scalar(NORMALIZATION_EPSILON)
            .sqrt();
        Self { mean, deviation }
    }
    pub(crate) fn normalise(&self, input: Tensor<5>) -> Tensor<5> {
        (input - self.mean.clone()) / self.deviation.clone()
    }
}
