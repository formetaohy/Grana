use grana_volume::{Basis, Level};

use crate::axes::Layout;

const INVERTIBLE: f64 = 1.0e-12;

pub(crate) fn levels(layouts: &[Layout]) -> Result<(Vec<Level>, [f32; 3]), String> {
    let first = layouts
        .first()
        .ok_or_else(|| "a multiscale image holds no level".to_string())?;
    let dims = [
        f64::from(first.extent.x()),
        f64::from(first.extent.y()),
        f64::from(first.extent.z()),
    ];
    let mut placement = first.origin;
    for (axis, column) in first.columns.iter().enumerate() {
        let weight = 0.5 * (dims[axis] - 1.0);
        for (component, value) in column.iter().enumerate() {
            placement[component] += weight * value;
        }
    }
    let placement = placement.map(|value| value as f32);
    if !placement.iter().all(|value| value.is_finite()) {
        return Err("the image placement is outside the supported range".to_string());
    }
    let levels = layouts
        .iter()
        .map(|layout| level(layout, placement))
        .collect::<Result<Vec<_>, String>>()?;
    Ok((levels, placement))
}

fn level(layout: &Layout, placement: [f32; 3]) -> Result<Level, String> {
    let mut corner = layout.origin;
    for (component, slot) in corner.iter_mut().enumerate() {
        let mut shift = f64::from(placement[component]);
        for column in &layout.columns {
            shift += 0.5 * column[component];
        }
        *slot -= shift;
    }
    let determinant = determinant(&layout.columns);
    let volume = (0..3)
        .map(|axis| length(&layout.columns[axis]))
        .product::<f64>();
    if !determinant.is_finite() || determinant.abs() <= INVERTIBLE * volume || volume <= 0.0 {
        return Err("an image level holds a singular placement".to_string());
    }
    let mut columns = [[0.0f32; 3]; 3];
    for (axis, column) in layout.columns.iter().enumerate() {
        for (component, value) in column.iter().enumerate() {
            let value = *value as f32;
            if !value.is_finite() {
                return Err(
                    "an image level holds a placement outside the supported range".to_string(),
                );
            }
            columns[axis][component] = value;
        }
    }
    let corner = corner.map(|value| value as f32);
    if !corner.iter().all(|value| value.is_finite()) {
        return Err("an image level holds an origin outside the supported range".to_string());
    }
    Ok(Level::placed(layout.extent, Basis::new(columns), corner))
}

fn determinant(columns: &[[f64; 3]; 3]) -> f64 {
    let [x, y, z] = columns;
    x[0] * (y[1] * z[2] - y[2] * z[1]) - y[0] * (x[1] * z[2] - x[2] * z[1])
        + z[0] * (x[1] * y[2] - x[2] * y[1])
}

fn length(vector: &[f64; 3]) -> f64 {
    vector.iter().map(|value| value * value).sum::<f64>().sqrt()
}
