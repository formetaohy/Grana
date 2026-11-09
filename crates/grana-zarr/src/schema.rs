use grana_volume::Level;
use serde_json::{Value, json};

pub(crate) const VERSION: &str = "0.6";
pub(crate) const INTRINSIC: &str = "physical";
pub(crate) const DISPLAY: &str = "stage";

const ALIGNMENT_TOLERANCE: f64 = 1.0e-6;

const PERMUTATION: [[f64; 3]; 3] = [[0.0, 0.0, 1.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]];

pub(crate) fn axes() -> Vec<Value> {
    vec![
        json!({ "name": "t", "type": "time", "unit": "second" }),
        json!({ "name": "c", "type": "channel" }),
        json!({ "name": "z", "type": "space", "unit": "micrometer" }),
        json!({ "name": "y", "type": "space", "unit": "micrometer" }),
        json!({ "name": "x", "type": "space", "unit": "micrometer" }),
    ]
}

fn display_axes() -> Vec<Value> {
    vec![
        json!({ "name": "x", "type": "space", "unit": "micrometer" }),
        json!({ "name": "y", "type": "space", "unit": "micrometer" }),
        json!({ "name": "z", "type": "space", "unit": "micrometer" }),
    ]
}

pub(crate) fn rotation(level: Level) -> [[f64; 3]; 3] {
    let basis = level.basis();
    let mut steps = [[0.0f64; 3]; 3];
    for (axis, step) in steps.iter_mut().enumerate() {
        let column = basis.column(axis);
        let length = column
            .iter()
            .map(|value| f64::from(*value) * f64::from(*value))
            .sum::<f64>()
            .sqrt();
        *step = column.map(|value| f64::from(value) / length);
    }
    [
        [steps[2][0], steps[1][0], steps[0][0]],
        [steps[2][1], steps[1][1], steps[0][1]],
        [steps[2][2], steps[1][2], steps[0][2]],
    ]
}

pub(crate) fn rotated(level: Level) -> bool {
    let matrix = rotation(level);
    (0..3).any(|row| {
        (0..3).any(|column| {
            (matrix[row][column] - PERMUTATION[row][column]).abs() > ALIGNMENT_TOLERANCE
        })
    })
}

pub(crate) fn multiscales(
    name: &str,
    axes: &[Value],
    datasets: Vec<Value>,
    rotation: Option<[[f64; 3]; 3]>,
) -> Value {
    match rotation {
        Some(rotation) => {
            let transform = if determinant(&rotation) < 0.0 {
                json!({
                    "type": "affine",
                    "affine": rotation.map(|row| [row[0], row[1], row[2], 0.0]),
                })
            } else {
                json!({
                    "type": "rotation",
                    "rotation": rotation,
                })
            };
            json!([{
                "name": name,
                "coordinateSystems": [
                    { "name": INTRINSIC, "axes": axes },
                    { "name": DISPLAY, "axes": display_axes() },
                ],
                "datasets": datasets,
                "coordinateTransformations": [{
                    "type": "sequence",
                    "input": { "name": INTRINSIC },
                    "output": { "name": DISPLAY },
                    "transformations": [
                        { "type": "projectAxis", "droppedInputs": [0, 1], "createdOutputs": [] },
                        transform,
                    ],
                }],
            }])
        }
        None => json!([{
            "name": name,
            "coordinateSystems": [{ "name": INTRINSIC, "axes": axes }],
            "datasets": datasets,
        }]),
    }
}

fn determinant(columns: &[[f64; 3]; 3]) -> f64 {
    let [x, y, z] = columns;
    x[0] * (y[1] * z[2] - y[2] * z[1]) - y[0] * (x[1] * z[2] - x[2] * z[1])
        + z[0] * (x[1] * y[2] - x[2] * y[1])
}

pub(crate) fn dataset(path: &str, scale: &[f64], translation: &[f64]) -> Value {
    json!({
        "path": path,
        "coordinateTransformations": [{
            "type": "sequence",
            "input": { "path": path },
            "output": { "name": INTRINSIC },
            "transformations": [
                { "type": "scale", "scale": scale },
                { "type": "translation", "translation": translation },
            ],
        }],
    })
}
