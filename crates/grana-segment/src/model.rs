use std::collections::HashSet;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use half::f16;
use ort::environment::Environment;
use ort::session::{Session, SessionOutputs};
use ort::value::{DynValue, Outlet, Tensor, TensorElementType, ValueType};

use crate::Result;
use crate::dynamics::Flow;
use crate::error::Error;

static ENVIRONMENT: OnceLock<Mutex<Option<Environment>>> = OnceLock::new();

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Precision {
    F32,
    F16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelInfo {
    name: String,
    side: u32,
    precision: Precision,
    outputs: usize,
}

impl ModelInfo {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn side(&self) -> u32 {
        self.side
    }

    pub fn precision(&self) -> Precision {
        self.precision
    }

    pub fn outputs(&self) -> usize {
        self.outputs
    }
}

#[derive(Clone, Copy, Debug)]
enum OutputLayout {
    Combined(usize),
    Split { flow: usize, cellprob: usize },
}

pub struct Model {
    session: Session,
    info: ModelInfo,
    input_name: String,
    output_names: Vec<String>,
    layout: OutputLayout,
}

impl Model {
    pub fn open(path: &Path) -> Result<Model> {
        let mut environment = environment_slot()
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if environment.is_none() {
            let built = ort::init()
                .build()
                .map_err(|error| Error::model(format!("ONNX Runtime cannot start: {error}")))?;
            *environment = Some(built);
        }
        let threads = std::thread::available_parallelism()
            .map(|value| value.get())
            .unwrap_or(1);
        let mut builder = Session::builder(
            environment
                .as_ref()
                .expect("the environment was initialized above"),
        )
        .map_err(|error| Error::model(format!("the session cannot be configured: {error}")))?
        .with_intra_threads(threads)
        .map_err(|error| Error::model(format!("the session cannot be configured: {error}")))?;
        let session = builder.commit_from_file(path).map_err(|error| {
            Error::model(format!(
                "the model `{}` cannot load: {error}",
                path.display()
            ))
        })?;
        drop(environment);

        let inputs = session.inputs();
        if inputs.len() != 1 {
            return Err(Error::model(format!(
                "the model must declare exactly one input tensor, found {}",
                inputs.len()
            )));
        }
        let input_name = inputs[0].name().to_owned();
        if input_name.is_empty() {
            return Err(Error::model("the model input name is empty"));
        }
        let (input_type, input_shape) = tensor_signature(&inputs[0])?;
        let precision = precision_of(input_type)?;
        if input_shape.len() != 4
            || input_shape[0] != 1
            || input_shape[1] != 3
            || input_shape[2] != input_shape[3]
        {
            return Err(Error::model(format!(
                "the model input shape must be [1, 3, side, side], found {input_shape:?}"
            )));
        }
        let side = input_shape[2];
        if !matches!(side, 128 | 256 | 384 | 512) {
            return Err(Error::model(format!(
                "the model input side must be 128, 256, 384 or 512, found {side}"
            )));
        }
        let side = side as u32;

        let outputs = session.outputs();
        if outputs.is_empty() {
            return Err(Error::model("the model declares no outputs"));
        }
        let mut names = HashSet::new();
        for output in outputs {
            if output.name().is_empty() {
                return Err(Error::model("the model output name is empty"));
            }
            let (output_type, _) = tensor_signature(output)?;
            if !matches!(
                output_type,
                TensorElementType::Float32 | TensorElementType::Float16
            ) {
                return Err(Error::model(format!(
                    "the model output `{}` has unsupported type {output_type:?}",
                    output.name()
                )));
            }
            if !names.insert(output.name().to_owned()) {
                return Err(Error::model(format!(
                    "the model output name `{}` is duplicated",
                    output.name()
                )));
            }
        }
        let output_names: Vec<String> = outputs
            .iter()
            .map(|output| output.name().to_owned())
            .collect();
        let layout = output_layout(outputs, side)?;
        let info = ModelInfo {
            name: path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string()),
            side,
            precision,
            outputs: output_names.len(),
        };
        Ok(Model {
            session,
            info,
            input_name,
            output_names,
            layout,
        })
    }

    pub fn info(&self) -> &ModelInfo {
        &self.info
    }

    pub fn infer(&mut self, tile: &[f32]) -> Result<Flow> {
        let side = self.info.side;
        let area = side as usize * side as usize;
        let input_length = area * 3;
        if tile.len() != input_length {
            return Err(Error::input(format!(
                "the model tile must contain {input_length} values, found {}",
                tile.len()
            )));
        }
        let shape = [1usize, 3, side as usize, side as usize];
        let input_name = self.input_name.clone();
        let outputs = match self.info.precision {
            Precision::F32 => {
                let tensor = Tensor::from_array((shape, tile.to_vec())).map_err(|error| {
                    Error::inference(format!("the input tensor is invalid: {error}"))
                })?;
                self.session
                    .run(ort::inputs![input_name.as_str() => tensor])
                    .map_err(|error| Error::inference(format!("the model run failed: {error}")))?
            }
            Precision::F16 => {
                let data: Vec<f16> = tile.iter().map(|&value| f16::from_f32(value)).collect();
                let tensor = Tensor::from_array((shape, data)).map_err(|error| {
                    Error::inference(format!("the input tensor is invalid: {error}"))
                })?;
                self.session
                    .run(ort::inputs![input_name.as_str() => tensor])
                    .map_err(|error| Error::inference(format!("the model run failed: {error}")))?
            }
        };

        let (dy, dx, cellprob) = match self.layout {
            OutputLayout::Combined(index) => {
                let values = output_values(named_output(&outputs, &self.output_names, index)?)?;
                if values.len() != area * 3 {
                    return Err(Error::inference(format!(
                        "the combined output must contain {} values, found {}",
                        area * 3,
                        values.len()
                    )));
                }
                let mut values = values;
                let cellprob = values.split_off(area * 2);
                let dx = values.split_off(area);
                (values, dx, cellprob)
            }
            OutputLayout::Split { flow, cellprob } => {
                let mut flow_values =
                    output_values(named_output(&outputs, &self.output_names, flow)?)?;
                let cellprob_values =
                    output_values(named_output(&outputs, &self.output_names, cellprob)?)?;
                if flow_values.len() != area * 2 {
                    return Err(Error::inference(format!(
                        "the flow output must contain {} values, found {}",
                        area * 2,
                        flow_values.len()
                    )));
                }
                if cellprob_values.len() != area {
                    return Err(Error::inference(format!(
                        "the cellprob output must contain {area} values, found {}",
                        cellprob_values.len()
                    )));
                }
                let dx = flow_values.split_off(area);
                (flow_values, dx, cellprob_values)
            }
        };
        if dy
            .iter()
            .chain(dx.iter())
            .chain(cellprob.iter())
            .any(|value| !value.is_finite())
        {
            return Err(Error::inference(
                "the model produced non-finite flow values",
            ));
        }
        Ok(Flow::from_parts(side, dy, dx, cellprob))
    }
}

fn environment_slot() -> &'static Mutex<Option<Environment>> {
    ENVIRONMENT.get_or_init(|| Mutex::new(None))
}

fn tensor_signature(outlet: &Outlet) -> Result<(TensorElementType, Vec<i64>)> {
    match outlet.dtype() {
        ValueType::Tensor { ty, shape, .. } => Ok((*ty, shape.to_vec())),
        dtype => Err(Error::model(format!(
            "the model value `{}` is not a tensor: {dtype}",
            outlet.name()
        ))),
    }
}

fn precision_of(value_type: TensorElementType) -> Result<Precision> {
    match value_type {
        TensorElementType::Float32 => Ok(Precision::F32),
        TensorElementType::Float16 => Ok(Precision::F16),
        other => Err(Error::model(format!(
            "the model input has unsupported type {other:?}"
        ))),
    }
}

fn output_layout(outputs: &[Outlet], side: u32) -> Result<OutputLayout> {
    let side = i64::from(side);
    match outputs.len() {
        1 => {
            let (_, shape) = tensor_signature(&outputs[0])?;
            if !is_combined_shape(&shape, side) {
                return Err(Error::model(format!(
                    "the model combined output shape must be [1, 3, {side}, {side}], found {:?}",
                    shape
                )));
            }
            Ok(OutputLayout::Combined(0))
        }
        2 => {
            let mut flow = None;
            let mut cellprob = None;
            for (index, output) in outputs.iter().enumerate() {
                let (_, shape) = tensor_signature(output)?;
                if is_flow_shape(&shape, side) {
                    if flow.replace(index).is_some() {
                        return Err(Error::model("the model declares more than one flow output"));
                    }
                } else if is_cellprob_shape(&shape, side) {
                    if cellprob.replace(index).is_some() {
                        return Err(Error::model(
                            "the model declares more than one cellprob output",
                        ));
                    }
                } else {
                    return Err(Error::model(format!(
                        "the model output `{}` has unsupported shape {:?}",
                        output.name(),
                        shape
                    )));
                }
            }
            match (flow, cellprob) {
                (Some(flow), Some(cellprob)) => Ok(OutputLayout::Split { flow, cellprob }),
                _ => Err(Error::model(
                    "the model must declare one flow output and one cellprob output",
                )),
            }
        }
        count => Err(Error::model(format!(
            "the model must declare one or two outputs, found {count}"
        ))),
    }
}

fn is_combined_shape(shape: &[i64], side: i64) -> bool {
    shape.len() == 4 && shape[0] == 1 && shape[1] == 3 && shape[2] == side && shape[3] == side
}

fn is_flow_shape(shape: &[i64], side: i64) -> bool {
    shape.len() == 4 && shape[0] == 1 && shape[1] == 2 && shape[2] == side && shape[3] == side
}

fn is_cellprob_shape(shape: &[i64], side: i64) -> bool {
    (shape.len() == 4 && shape[0] == 1 && shape[1] == 1 && shape[2] == side && shape[3] == side)
        || (shape.len() == 3 && shape[0] == 1 && shape[1] == side && shape[2] == side)
}

fn named_output<'a>(
    outputs: &'a SessionOutputs<'_>,
    names: &[String],
    index: usize,
) -> Result<&'a DynValue> {
    let name = names
        .get(index)
        .expect("the output layout indexes a declared output");
    outputs
        .get(name)
        .ok_or_else(|| Error::inference(format!("the model produced no output named `{name}`")))
}

fn output_values(value: &DynValue) -> Result<Vec<f32>> {
    match value.dtype() {
        ValueType::Tensor {
            ty: TensorElementType::Float32,
            ..
        } => {
            let (_, data) = value
                .try_extract_tensor::<f32>()
                .map_err(|error| Error::inference(format!("the output cannot be read: {error}")))?;
            Ok(data.to_vec())
        }
        ValueType::Tensor {
            ty: TensorElementType::Float16,
            ..
        } => {
            let (_, data) = value
                .try_extract_tensor::<f16>()
                .map_err(|error| Error::inference(format!("the output cannot be read: {error}")))?;
            Ok(data.iter().map(|value| value.to_f32()).collect())
        }
        dtype => Err(Error::inference(format!(
            "the model output has unsupported type {dtype}"
        ))),
    }
}
