use serde_json::Value;

use crate::Error;

const INVERTIBLE: f64 = 1.0e-12;

#[derive(Clone, Debug)]
pub(crate) struct Affine {
    rows: usize,
    columns: usize,
    linear: Vec<f64>,
    offset: Vec<f64>,
}

impl Affine {
    pub fn identity(dimensions: usize) -> Self {
        let mut affine = Self::zero(dimensions, dimensions);
        for axis in 0..dimensions {
            affine.set(axis, axis, 1.0);
        }
        affine
    }

    pub fn zero(rows: usize, columns: usize) -> Self {
        Self {
            rows,
            columns,
            linear: vec![0.0; rows * columns],
            offset: vec![0.0; rows],
        }
    }

    pub fn scale(values: &[f64]) -> Self {
        let mut affine = Self::zero(values.len(), values.len());
        for (axis, value) in values.iter().enumerate() {
            affine.set(axis, axis, *value);
        }
        affine
    }

    pub fn translation(values: &[f64]) -> Self {
        let mut affine = Self::zero(values.len(), values.len());
        for (axis, value) in values.iter().enumerate() {
            affine.set(axis, axis, 1.0);
            affine.offset[axis] = *value;
        }
        affine
    }

    pub fn rotation(matrix: &[Vec<f64>]) -> Result<Self, String> {
        let dimensions = matrix.len();
        if matrix.iter().any(|row| row.len() != dimensions) {
            return Err("a rotation matrix is square".to_string());
        }
        let mut affine = Self::zero(dimensions, dimensions);
        for (row, values) in matrix.iter().enumerate() {
            for (column, value) in values.iter().enumerate() {
                affine.set(row, column, *value);
            }
        }
        Ok(affine)
    }

    pub fn map_axis(permutation: &[usize]) -> Result<Self, String> {
        let dimensions = permutation.len();
        let mut seen = vec![false; dimensions];
        for axis in permutation {
            if *axis >= dimensions || seen[*axis] {
                return Err("an axis permutation names every axis exactly once".to_string());
            }
            seen[*axis] = true;
        }
        let mut affine = Self::zero(dimensions, dimensions);
        for (output, input) in permutation.iter().enumerate() {
            affine.set(output, *input, 1.0);
        }
        Ok(affine)
    }

    pub fn project(
        input: usize,
        output: usize,
        dropped: &[usize],
        created: &[usize],
    ) -> Result<Self, String> {
        let mut kept = Vec::with_capacity(input);
        for axis in 0..input {
            if !dropped.contains(&axis) {
                kept.push(axis);
            }
        }
        if dropped.iter().any(|axis| *axis >= input) {
            return Err("a dropped input axis lies outside the input".to_string());
        }
        if created.iter().any(|axis| *axis >= output) {
            return Err("a created output axis lies outside the output".to_string());
        }
        if kept.len() + created.len() != output {
            return Err(format!(
                "a projection keeps {} and creates {} axes for {output} outputs",
                kept.len(),
                created.len()
            ));
        }
        let mut affine = Self::zero(output, input);
        let mut kept = kept.into_iter();
        for axis in 0..output {
            if created.contains(&axis) {
                continue;
            }
            let input = kept
                .next()
                .ok_or_else(|| "a projection lists fewer kept axes than outputs".to_string())?;
            affine.set(axis, input, 1.0);
        }
        Ok(affine)
    }

    pub fn columns(&self) -> usize {
        self.columns
    }

    pub fn linear(&self, row: usize, column: usize) -> f64 {
        self.linear[row * self.columns + column]
    }

    pub fn offset(&self, row: usize) -> f64 {
        self.offset[row]
    }

    pub fn set(&mut self, row: usize, column: usize, value: f64) {
        self.linear[row * self.columns + column] = value;
    }

    pub fn add_offset(&mut self, row: usize, value: f64) {
        self.offset[row] += value;
    }

    pub fn composite(&self, inner: &Affine) -> Result<Affine, String> {
        if self.columns != inner.rows {
            return Err(format!(
                "a transform of {} columns meets a transform of {} rows",
                self.columns, inner.rows
            ));
        }
        let mut result = Affine::zero(self.rows, inner.columns);
        for row in 0..self.rows {
            for column in 0..inner.columns {
                let mut value = 0.0;
                for shared in 0..self.columns {
                    value += self.linear(row, shared) * inner.linear(shared, column);
                }
                result.set(row, column, value);
            }
            let mut value = self.offset(row);
            for shared in 0..self.columns {
                value += self.linear(row, shared) * inner.offset(shared);
            }
            result.add_offset(row, value);
        }
        Ok(result)
    }

    pub fn inverse(&self) -> Result<Affine, String> {
        if self.rows != self.columns {
            return Err("only a square transform is invertible".to_string());
        }
        let dimensions = self.rows;
        let mut matrix = vec![0.0; dimensions * 2 * dimensions];
        for row in 0..dimensions {
            for column in 0..dimensions {
                matrix[row * 2 * dimensions + column] = self.linear(row, column);
            }
            matrix[row * 2 * dimensions + dimensions + row] = 1.0;
        }
        let width = 2 * dimensions;
        for pivot in 0..dimensions {
            let best = (pivot..dimensions)
                .max_by(|left, right| {
                    matrix[*left * width + pivot]
                        .abs()
                        .total_cmp(&matrix[*right * width + pivot].abs())
                })
                .expect("a pivot row exists");
            if matrix[best * width + pivot].abs() <= INVERTIBLE {
                return Err("a coordinate transform is not invertible".to_string());
            }
            if best != pivot {
                for column in 0..width {
                    matrix.swap(pivot * width + column, best * width + column);
                }
            }
            let scale = matrix[pivot * width + pivot];
            for column in 0..width {
                matrix[pivot * width + column] /= scale;
            }
            for row in 0..dimensions {
                if row == pivot {
                    continue;
                }
                let factor = matrix[row * width + pivot];
                if factor == 0.0 {
                    continue;
                }
                for column in 0..width {
                    matrix[row * width + column] -= factor * matrix[pivot * width + column];
                }
            }
        }
        let mut inverse = Affine::zero(dimensions, dimensions);
        for row in 0..dimensions {
            for column in 0..dimensions {
                inverse.set(row, column, matrix[row * width + dimensions + column]);
            }
        }
        let mut offset = vec![0.0; dimensions];
        for (row, slot) in offset.iter_mut().enumerate() {
            for column in 0..dimensions {
                *slot -= inverse.linear(row, column) * self.offset(column);
            }
        }
        for (row, value) in offset.into_iter().enumerate() {
            inverse.add_offset(row, value);
        }
        Ok(inverse)
    }
}

pub(crate) fn parse(
    path: &str,
    value: &Value,
    input: usize,
    output: usize,
) -> Result<Affine, Error> {
    let kind = value
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| detail(path, "a coordinate transformation names no type"))?;
    let unsupported = |detail: String| detail.into_error(path);
    let affine = match kind {
        "identity" => {
            require_same(input, output).map_err(unsupported)?;
            Affine::identity(input)
        }
        "scale" => {
            require_same(input, output).map_err(unsupported)?;
            Affine::scale(&numbers(path, value, "scale", input)?)
        }
        "translation" => {
            require_same(input, output).map_err(unsupported)?;
            Affine::translation(&numbers(path, value, "translation", input)?)
        }
        "affine" => {
            let rows = matrix(path, value, "affine")?;
            let mut affine = Affine::zero(output, input);
            if rows.len() != output || rows.iter().any(|row| row.len() != input + 1) {
                return Err(detail(
                    path,
                    format!("an affine transform is not {output} by {}", input + 1),
                ));
            }
            for (row, values) in rows.iter().enumerate() {
                for (column, value) in values.iter().take(input).enumerate() {
                    affine.set(row, column, *value);
                }
                affine.add_offset(row, values[input]);
            }
            affine
        }
        "rotation" => {
            require_same(input, output).map_err(unsupported)?;
            let rows = matrix(path, value, "rotation")?;
            Affine::rotation(&rows).map_err(unsupported)?
        }
        "mapAxis" => {
            require_same(input, output).map_err(unsupported)?;
            let permutation = value
                .get("mapAxis")
                .and_then(Value::as_array)
                .ok_or_else(|| detail(path, "a mapAxis transform holds no permutation"))?
                .iter()
                .map(|entry| {
                    entry
                        .as_u64()
                        .and_then(|axis| usize::try_from(axis).ok())
                        .ok_or_else(|| detail(path, "a mapAxis permutation is not an index"))
                })
                .collect::<Result<Vec<_>, Error>>()?;
            if permutation.len() != input {
                return Err(detail(
                    path,
                    format!(
                        "a mapAxis permutation orders {} axes for {input} inputs",
                        permutation.len()
                    ),
                ));
            }
            Affine::map_axis(&permutation).map_err(unsupported)?
        }
        "projectAxis" => {
            let dropped = indices(path, value, "droppedInputs")?;
            let created = indices(path, value, "createdOutputs")?;
            Affine::project(input, output, &dropped, &created).map_err(unsupported)?
        }
        "sequence" => {
            let transformations = value
                .get("transformations")
                .and_then(Value::as_array)
                .ok_or_else(|| detail(path, "a sequence transform lists no transforms"))?;
            let mut total = Affine::identity(input);
            let mut current = input;
            for entry in transformations {
                let (inner_input, inner_output) =
                    dimensions(path, entry, current, output).map_err(unsupported)?;
                let inner = parse(path, entry, inner_input, inner_output)?;
                total = inner.composite(&total).map_err(unsupported)?;
                current = inner_output;
            }
            if current != output {
                return Err(detail(
                    path,
                    format!("a sequence transform ends on {current} of {output} outputs"),
                ));
            }
            total
        }
        "byDimension" => {
            let transformations = value
                .get("transformations")
                .and_then(Value::as_array)
                .ok_or_else(|| detail(path, "a byDimension transform lists no transforms"))?;
            let mut total = Affine::zero(output, input);
            for entry in transformations {
                let inputs = indices(path, entry, "inputAxes")?;
                let outputs = indices(path, entry, "outputAxes")?;
                let inner = parse(path, entry, inputs.len(), outputs.len())?;
                for (row, output_axis) in outputs.iter().enumerate() {
                    if *output_axis >= output {
                        return Err(detail(path, "a byDimension output axis is out of range"));
                    }
                    for (column, input_axis) in inputs.iter().enumerate() {
                        if *input_axis >= input {
                            return Err(detail(path, "a byDimension input axis is out of range"));
                        }
                        total.set(*output_axis, *input_axis, inner.linear(row, column));
                    }
                    total.add_offset(*output_axis, inner.offset(row));
                }
            }
            total
        }
        "bijection" => {
            let forward = value
                .get("forward")
                .ok_or_else(|| detail(path, "a bijection transform names no forward"))?;
            parse(path, forward, input, output)?
        }
        "coordinates" | "displacements" => {
            return Err(Error::Unsupported {
                path: path.to_string(),
                detail: format!("the `{kind}` coordinate transformation is not supported"),
            });
        }
        other => {
            return Err(detail(
                path,
                format!("the coordinate transformation `{other}` is not supported"),
            ));
        }
    };
    Ok(affine)
}

trait Failure {
    fn into_error(self, path: &str) -> Error;
}

impl Failure for String {
    fn into_error(self, path: &str) -> Error {
        Error::Unsupported {
            path: path.to_string(),
            detail: self,
        }
    }
}

fn require_same(input: usize, output: usize) -> Result<(), String> {
    if input == output {
        Ok(())
    } else {
        Err(format!(
            "a transform of {input} dimensions cannot name {output} outputs"
        ))
    }
}

fn dimensions(
    path: &str,
    value: &Value,
    input: usize,
    output: usize,
) -> Result<(usize, usize), String> {
    let kind = value
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("a coordinate transformation in `{path}` names no type"))?;
    let count = |key: &str| {
        value
            .get(key)
            .and_then(Value::as_array)
            .map(|values| values.len())
    };
    Ok(match kind {
        "scale" | "translation" => {
            let dimensions = count(kind)
                .ok_or_else(|| format!("the `{kind}` transform of `{path}` holds no vector"))?;
            (dimensions, dimensions)
        }
        "mapAxis" => {
            let dimensions = count("mapAxis").ok_or_else(|| {
                format!("the `mapAxis` transform of `{path}` holds no permutation")
            })?;
            (dimensions, dimensions)
        }
        "rotation" => {
            let dimensions = value
                .get("rotation")
                .and_then(Value::as_array)
                .map(Vec::len)
                .ok_or_else(|| format!("the `rotation` transform of `{path}` holds no matrix"))?;
            (dimensions, dimensions)
        }
        "affine" => {
            let rows = value
                .get("affine")
                .and_then(Value::as_array)
                .ok_or_else(|| format!("the `affine` transform of `{path}` holds no matrix"))?;
            let columns = rows
                .first()
                .and_then(Value::as_array)
                .map(Vec::len)
                .unwrap_or(0);
            (columns.saturating_sub(1), rows.len())
        }
        "identity" => (input, input),
        "projectAxis" => {
            let dropped = count("droppedInputs").unwrap_or(0);
            let created = count("createdOutputs").unwrap_or(0);
            let kept = input
                .checked_sub(dropped)
                .ok_or_else(|| format!("a projection of `{path}` drops {dropped} of {input}"))?;
            (input, kept + created)
        }
        _ => (input, output),
    })
}

fn numbers(path: &str, value: &Value, key: &str, count: usize) -> Result<Vec<f64>, Error> {
    let values = value
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| detail(path, format!("the `{key}` transform holds no vector")))?;
    if values.len() != count {
        return Err(detail(
            path,
            format!(
                "the `{key}` transform holds {} components for {count} axes",
                values.len()
            ),
        ));
    }
    values
        .iter()
        .map(|value| {
            value
                .as_f64()
                .filter(|value| value.is_finite())
                .ok_or_else(|| detail(path, format!("the `{key}` transform is not finite")))
        })
        .collect()
}

fn matrix(path: &str, value: &Value, key: &str) -> Result<Vec<Vec<f64>>, Error> {
    let stored = value
        .get(key)
        .ok_or_else(|| detail(path, format!("the `{key}` transform holds no matrix")))?;
    let rows = stored
        .as_array()
        .ok_or_else(|| detail(path, format!("the `{key}` transform is not a matrix")))?;
    rows.iter()
        .map(|row| {
            row.as_array()
                .ok_or_else(|| detail(path, format!("a `{key}` matrix row is not an array")))?
                .iter()
                .map(|value| {
                    value
                        .as_f64()
                        .filter(|value| value.is_finite())
                        .ok_or_else(|| {
                            detail(path, format!("a `{key}` matrix entry is not finite"))
                        })
                })
                .collect()
        })
        .collect()
}

fn indices(path: &str, value: &Value, key: &str) -> Result<Vec<usize>, Error> {
    let values = value
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| detail(path, format!("a wrapper transform holds no `{key}`")))?;
    values
        .iter()
        .map(|entry| {
            entry
                .as_u64()
                .and_then(|axis| usize::try_from(axis).ok())
                .ok_or_else(|| detail(path, format!("a `{key}` entry is not an axis index")))
        })
        .collect()
}

fn detail(path: &str, detail: impl Into<String>) -> Error {
    Error::Metadata {
        path: path.to_string(),
        detail: detail.into(),
    }
}
