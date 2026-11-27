use burn::prelude::*;
use burn::tensor::{DType, Shape};
use burn_cubecl::Cube;
use burn_cubecl::cubecl::ir::{ElemType, FloatKind};
use burn_cubecl::ops::numeric::empty_device_dtype;
use burn_cubecl::tensor::CubeTensor;
use cubek::convolution::{
    AcceleratedTileKind, ConvAlgorithm, ConvolutionArgs, ConvolutionInputs, Strategy, launch_ref,
};
use cubek::matmul::definition::{MatmulElems, MatmulGlobalElems};
use cubek::std::InputBinding;

const TO_CHANNELS_LAST: [usize; 5] = [0, 2, 3, 4, 1];
const TO_CHANNELS_FIRST: [usize; 5] = [0, 4, 1, 2, 3];

pub(crate) fn channels_last(tensor: Tensor<5>) -> Tensor<5> {
    tensor.permute(TO_CHANNELS_LAST)
}

pub(crate) fn channels_last_values(values: &[f32], shape: [usize; 5]) -> Vec<f32> {
    let [output, input, depth, height, width] = shape;
    let mut reordered = vec![0f32; values.len()];
    for o in 0..output {
        for d in 0..depth {
            for h in 0..height {
                for w in 0..width {
                    for c in 0..input {
                        reordered[(((o * depth + d) * height + h) * width + w) * input + c] =
                            values[(((o * input + c) * depth + d) * height + h) * width + w];
                    }
                }
            }
        }
    }
    reordered
}

pub(crate) fn convolve(
    input: Tensor<5>,
    weight: Tensor<5>,
    bias: Tensor<1>,
    kernel: [usize; 3],
    padding: [usize; 3],
) -> Tensor<5> {
    let [batch, _, depth, height, width] = input.dims();
    let output = weight.dims()[0];
    let shape = [
        batch,
        convolved(depth, kernel[0], padding[0]),
        convolved(height, kernel[1], padding[1]),
        convolved(width, kernel[2], padding[2]),
        output,
    ];
    let input = primitive(channels_last(input));
    let weight = primitive(weight);
    let client = input.client.clone();
    let out = empty_device_dtype(
        client.clone(),
        input.device.clone(),
        Shape::from(shape.to_vec()),
        DType::F32,
    );
    let dtypes = flex32_globals();
    let strategy = Strategy::Inferred {
        algorithm: ConvAlgorithm::SimpleSyncCyclic,
        tile_kind: AcceleratedTileKind::Cmma,
    };
    launch_ref::<3>(
        &strategy,
        &client,
        ConvolutionInputs::Forward {
            input: InputBinding::new(input.binding(), dtypes.lhs_global),
            weight: InputBinding::new(weight.binding(), dtypes.rhs_global),
            bias: None,
            out: out.clone().binding(),
        },
        ConvolutionArgs {
            stride: [1, 1, 1],
            padding,
            dilation: [1, 1, 1],
        },
        dtypes,
    )
    .unwrap_or_else(|error| panic!("the implicit GEMM convolution launches: {error:?}"));
    let activated = tensor(out) + bias.reshape([1, 1, 1, 1, output]);
    activated.permute(TO_CHANNELS_FIRST)
}

fn flex32_globals() -> MatmulElems {
    MatmulElems::from_globals(&MatmulGlobalElems {
        lhs: ElemType::Float(FloatKind::Flex32),
        rhs: ElemType::Float(FloatKind::Flex32),
        out: ElemType::Float(FloatKind::F32),
    })
}

fn convolved(length: usize, kernel: usize, padding: usize) -> usize {
    length + 2 * padding - kernel + 1
}

fn primitive(tensor: Tensor<5>) -> CubeTensor {
    tensor
        .try_into_primitive::<Cube>()
        .expect("a segmentation tensor runs on the cube backend")
}

fn tensor(primitive: CubeTensor) -> Tensor<5> {
    Tensor::<5>::from_primitive::<Cube>(primitive)
}
