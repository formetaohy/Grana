use grana_volume::Level;
use serde_json::{Value, json};

pub(crate) const VERSION: &str = "0.6";
pub(crate) const INTRINSIC: &str = "physical";
pub(crate) const DISPLAY: &str = "stage";

const ALIGNMENT_TOLERANCE: f64 = 1.0e-6;

const PERMUTATION: [[f64; 3]; 3] = [[0.0, 0.0, 1.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]];

const PLANE_PERMUTATION: [[f64; 3]; 3] = [[0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 0.0]];

pub(crate) fn axes() -> Vec<Value> {
    vec![
        json!({ "name": "t", "type": "time", "unit": "second" }),
        json!({ "name": "c", "type": "channel" }),
        json!({ "name": "z", "type": "space", "unit": "micrometer" }),
        json!({ "name": "y", "type": "space", "unit": "micrometer" }),
        json!({ "name": "x", "type": "space", "unit": "micrometer" }),
    ]
}

fn display_axes(spatial: usize) -> Vec<Value> {
    ["x", "y", "z"][..spatial]
        .iter()
        .map(|name| json!({ "name": name, "type": "space", "unit": "micrometer" }))
        .collect()
}

pub(crate) fn placement(level: Level, spatial: usize) -> [[f64; 3]; 3] {
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
    let mut matrix = [[0.0; 3]; 3];
    for row in 0..spatial {
        for column in 0..spatial {
            matrix[row][column] = steps[spatial - 1 - column][row];
        }
    }
    matrix
}

pub(crate) fn rotated(level: Level, spatial: usize) -> bool {
    let matrix = placement(level, spatial);
    let reference = match spatial {
        2 => PLANE_PERMUTATION,
        _ => PERMUTATION,
    };
    (0..spatial).any(|row| {
        (0..spatial).any(|column| {
            (matrix[row][column] - reference[row][column]).abs() > ALIGNMENT_TOLERANCE
        })
    })
}

fn orthonormal(matrix: [[f64; 3]; 3], spatial: usize) -> bool {
    (0..spatial).all(|left| {
        (0..spatial).all(|right| {
            let dot = (0..spatial)
                .map(|row| matrix[row][left] * matrix[row][right])
                .sum::<f64>();
            let expected = f64::from(u8::from(left == right));
            (dot - expected).abs() <= ALIGNMENT_TOLERANCE
        })
    })
}

pub(crate) fn inverse(matrix: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let column = |axis: usize| [matrix[0][axis], matrix[1][axis], matrix[2][axis]];
    let cross = |left: [f64; 3], right: [f64; 3]| {
        [
            left[1] * right[2] - left[2] * right[1],
            left[2] * right[0] - left[0] * right[2],
            left[0] * right[1] - left[1] * right[0],
        ]
    };
    let first = column(0);
    let second = column(1);
    let third = column(2);
    let normal = cross(second, third);
    let determinant = first[0] * normal[0] + first[1] * normal[1] + first[2] * normal[2];
    assert!(determinant.abs() > 1.0e-12, "an exported placement inverts");
    [
        cross(second, third),
        cross(third, first),
        cross(first, second),
    ]
    .map(|row| row.map(|value| value / determinant))
}

pub(crate) struct Display {
    pub(crate) matrix: [[f64; 3]; 3],
    pub(crate) dropped: Vec<usize>,
    pub(crate) spatial: usize,
}

pub(crate) fn multiscales(
    name: &str,
    axes: &[Value],
    datasets: Vec<Value>,
    display: Option<Display>,
) -> Value {
    let Some(display) = display else {
        return json!([{
            "name": name,
            "coordinateSystems": [{ "name": INTRINSIC, "axes": axes }],
            "datasets": datasets,
        }]);
    };
    let Display {
        matrix,
        dropped,
        spatial,
    } = display;
    let rows = (0..spatial)
        .map(|row| {
            (0..spatial)
                .map(|column| matrix[row][column])
                .chain(std::iter::once(0.0))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let transform = if orthonormal(matrix, spatial) && determinant(&matrix, spatial) > 0.0 {
        json!({
            "type": "rotation",
            "rotation": (0..spatial)
                .map(|row| (0..spatial).map(|column| matrix[row][column]).collect::<Vec<_>>())
                .collect::<Vec<_>>(),
        })
    } else {
        json!({ "type": "affine", "affine": rows })
    };
    json!([{
        "name": name,
        "coordinateSystems": [
            { "name": INTRINSIC, "axes": axes },
            { "name": DISPLAY, "axes": display_axes(spatial) },
        ],
        "datasets": datasets,
        "coordinateTransformations": [{
            "type": "sequence",
            "input": { "name": INTRINSIC },
            "output": { "name": DISPLAY },
            "transformations": [
                { "type": "projectAxis", "droppedInputs": dropped, "createdOutputs": [] },
                transform,
            ],
        }],
    }])
}

fn determinant(columns: &[[f64; 3]; 3], spatial: usize) -> f64 {
    if spatial == 2 {
        return columns[0][0] * columns[1][1] - columns[0][1] * columns[1][0];
    }
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
