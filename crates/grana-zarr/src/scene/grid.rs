use grana_volume::{Basis, Extent, Level};

use crate::Error;
use crate::metadata;

use super::member::Member;

pub(super) const EXTENT_LIMIT: u32 = 1 << 20;

pub(super) const EPSILON: f64 = 1.0e-9;

pub(super) struct Grid {
    pub(super) levels: Vec<Level>,
    pub(super) placement: [f32; 3],
    pub(super) shift: [f64; 3],
}

pub(super) fn grid(
    path: &str,
    members: &[Member],
    reference: usize,
    stored: usize,
) -> Result<Grid, Error> {
    let anchor = &members[reference];
    let basis = multiply(anchor.spatial.linear, anchor.geometry[0].columns);
    let origin = apply(
        anchor.spatial.linear,
        anchor.spatial.offset,
        anchor.geometry[0].origin,
    );
    let reverse = invert(basis).ok_or_else(|| Error::Unsupported {
        path: path.to_string(),
        detail: "the scene grid is not invertible".to_string(),
    })?;
    let mut low = [f64::INFINITY; 3];
    let mut high = [f64::NEG_INFINITY; 3];
    for member in members {
        let geometry = &member.geometry[0];
        let placed = multiply(member.spatial.linear, geometry.columns);
        let corner = apply(
            member.spatial.linear,
            member.spatial.offset,
            geometry.origin,
        );
        let extent = geometry.extent;
        for slot in 0..8usize {
            let index = [0usize, 1, 2].map(|axis| {
                if slot & (1 << axis) == 0 {
                    0.0
                } else {
                    f64::from([extent.x(), extent.y(), extent.z()][axis])
                }
            });
            let world = apply(placed, corner, index);
            let at = apply(
                reverse,
                [0.0; 3],
                [
                    world[0] - origin[0],
                    world[1] - origin[1],
                    world[2] - origin[2],
                ],
            );
            for axis in 0..3 {
                low[axis] = low[axis].min(at[axis]);
                high[axis] = high[axis].max(at[axis]);
            }
        }
    }
    let shift = low.map(f64::floor);
    let reach = [0usize, 1, 2].map(|axis| high[axis].ceil() - shift[axis]);
    let mut extent = [0u32; 3];
    for axis in 0..3 {
        if !(reach[axis] >= 1.0 && reach[axis] <= f64::from(EXTENT_LIMIT)) {
            return Err(Error::Unsupported {
                path: path.to_string(),
                detail: format!(
                    "the scene spans {} voxels along an axis, outside the supported range",
                    reach[axis]
                ),
            });
        }
        extent[axis] = reach[axis] as u32;
    }
    let placed = apply(basis, origin, shift);
    let extent = Extent::new(extent[0], extent[1], extent[2]);
    let mut placement = placed;
    for (axis, reach) in [extent.x(), extent.y(), extent.z()].into_iter().enumerate() {
        let weight = 0.5 * f64::from(reach - 1);
        for (component, column) in basis.iter().enumerate() {
            placement[component] += weight * column[axis];
        }
    }
    let mut centered = [0.0; 3];
    for component in 0..3 {
        let column = basis[component];
        let shift = column[0] + column[1] + column[2];
        centered[component] = placed[component] - placement[component] - 0.5 * shift;
    }
    let shift = [0usize, 1, 2].map(|component| placed[component] - centered[component]);
    let first = Level::placed(
        extent,
        basis_of(path, basis)?,
        centered.map(|value| value as f32),
    );
    let mut levels = Vec::with_capacity(stored);
    levels.push(first);
    while levels.len() < stored {
        let last = *levels.last().expect("a grid holds a level");
        levels.push(last.coarser());
    }
    Ok(Grid {
        levels,
        placement: placement.map(|value| value as f32),
        shift,
    })
}

pub(super) fn basis_of(path: &str, matrix: [[f64; 3]; 3]) -> Result<Basis, Error> {
    let mut columns = [[0.0f32; 3]; 3];
    for (axis, column) in columns.iter_mut().enumerate() {
        for (component, value) in column.iter_mut().enumerate() {
            let held = matrix[component][axis];
            if !held.is_finite() || held.abs() > f64::from(f32::MAX) {
                return Err(Error::Unsupported {
                    path: path.to_string(),
                    detail: "the scene placement is outside the supported range".to_string(),
                });
            }
            *value = held as f32;
        }
    }
    Ok(Basis::new(columns))
}

pub(super) fn basis_matrix(basis: Basis) -> [[f64; 3]; 3] {
    let mut matrix = [[0.0; 3]; 3];
    for (axis, column) in [basis.column(0), basis.column(1), basis.column(2)]
        .into_iter()
        .enumerate()
    {
        for (component, value) in column.into_iter().enumerate() {
            matrix[component][axis] = f64::from(value);
        }
    }
    matrix
}

pub(super) fn dataset_columns(dataset: &metadata::Dataset) -> [[f64; 3]; 3] {
    let mut matrix = [[0.0; 3]; 3];
    for (axis, column) in dataset.columns.iter().enumerate() {
        for (component, value) in column.iter().enumerate() {
            matrix[component][axis] = *value;
        }
    }
    matrix
}

pub(super) fn multiply(left: [[f64; 3]; 3], right: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let mut product = [[0.0; 3]; 3];
    for row in 0..3 {
        for column in 0..3 {
            product[row][column] = (0..3)
                .map(|shared| left[row][shared] * right[shared][column])
                .sum();
        }
    }
    product
}

pub(super) fn apply(linear: [[f64; 3]; 3], offset: [f64; 3], point: [f64; 3]) -> [f64; 3] {
    let mut result = offset;
    for (axis, value) in point.into_iter().enumerate() {
        for component in 0..3 {
            result[component] += linear[component][axis] * value;
        }
    }
    result
}

pub(super) fn invert(matrix: [[f64; 3]; 3]) -> Option<[[f64; 3]; 3]> {
    let [x, y, z] = [
        [matrix[0][0], matrix[1][0], matrix[2][0]],
        [matrix[0][1], matrix[1][1], matrix[2][1]],
        [matrix[0][2], matrix[1][2], matrix[2][2]],
    ];
    let cross = |left: [f64; 3], right: [f64; 3]| {
        [
            left[1] * right[2] - left[2] * right[1],
            left[2] * right[0] - left[0] * right[2],
            left[0] * right[1] - left[1] * right[0],
        ]
    };
    let normal = cross(y, z);
    let determinant = x[0] * normal[0] + x[1] * normal[1] + x[2] * normal[2];
    if !determinant.is_finite() || determinant.abs() <= EPSILON {
        return None;
    }
    let rows = [normal, cross(z, x), cross(x, y)];
    let mut inverse = [[0.0; 3]; 3];
    for (row, held) in rows.iter().enumerate() {
        for (axis, value) in held.iter().enumerate() {
            inverse[row][axis] = value / determinant;
        }
    }
    Some(inverse)
}

pub(super) fn determinant(matrix: [[f64; 3]; 3]) -> f64 {
    matrix[0][0] * (matrix[1][1] * matrix[2][2] - matrix[1][2] * matrix[2][1])
        - matrix[0][1] * (matrix[1][0] * matrix[2][2] - matrix[1][2] * matrix[2][0])
        + matrix[0][2] * (matrix[1][0] * matrix[2][1] - matrix[1][1] * matrix[2][0])
}

pub(super) fn metadata_error(path: &str, detail: impl Into<String>) -> Error {
    Error::Metadata {
        path: path.to_string(),
        detail: detail.into(),
    }
}
