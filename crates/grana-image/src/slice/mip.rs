use grana_volume::{NoData, Reduction, VoxelData, VoxelValue};

use crate::slice::Plane;

pub(crate) fn reduce(
    lower: &Plane,
    upper: &Plane,
    width: u32,
    height: u32,
    reduction: Reduction,
    fill: NoData,
) -> Plane {
    assert_eq!(
        lower.width(),
        upper.width(),
        "the two slices of a reduction share their width"
    );
    assert_eq!(
        lower.height(),
        upper.height(),
        "the two slices of a reduction share their height"
    );
    assert!(
        width <= lower.width() && height <= lower.height(),
        "a reduced slice holds fewer pixels than the slices it averages"
    );
    let declared = fill.declared();
    let format = lower.data().format();
    assert_eq!(
        format,
        upper.data().format(),
        "the two slices of a reduction share their format"
    );
    let child = [lower.width(), lower.height()];
    macro_rules! arms {
        ($($variant:ident),*) => {
            match (lower.data(), upper.data()) {
                $(
                    (VoxelData::$variant(lower), VoxelData::$variant(upper)) => {
                        let data = reduced(
                            lower,
                            upper,
                            child[0],
                            child[1],
                            width,
                            height,
                            reduction,
                            match fill {
                                NoData::$variant(value) => value,
                                _ => unreachable!("a reduction fills with the format of its level"),
                            },
                            declared,
                        );
                        Plane::new(width, height, VoxelData::$variant(data))
                    }
                )*
                _ => panic!("the two slices of a reduction share their format"),
            }
        };
    }
    arms!(U8, U16, U32, U64, I8, I16, I32, I64, F32, F64)
}

fn index(width: u32, height: u32, x: u32, y: u32) -> usize {
    assert!(x < width && y < height, "a sample lies inside its slice");
    x as usize + width as usize * y as usize
}

#[allow(clippy::too_many_arguments)]
fn reduced<T: VoxelValue>(
    lower: &[T],
    upper: &[T],
    width: u32,
    height: u32,
    out_width: u32,
    out_height: u32,
    reduction: Reduction,
    fill: T,
    no_data: Option<NoData>,
) -> Vec<T> {
    let mut held = Vec::with_capacity((out_width as usize) * (out_height as usize));
    for y in 0..out_height {
        for x in 0..out_width {
            let base = (2 * x).min(width - 1);
            let line = (2 * y).min(height - 1);
            let mut values = [fill; 8];
            let mut present = 0usize;
            for slice in [lower, upper] {
                for offset in 0..2u32 {
                    let at = (line + offset).min(height - 1);
                    for step in 0..2u32 {
                        let value = slice[index(width, height, (base + step).min(width - 1), at)];
                        if no_data.is_some_and(|no_data| value.is_no_data(no_data)) {
                            continue;
                        }
                        values[present] = value;
                        present += 1;
                    }
                }
            }
            held.push(if present == 0 {
                fill
            } else {
                reduce_values(&values[..present], reduction, fill)
            });
        }
    }
    held
}

fn reduce_values<T: VoxelValue>(values: &[T], reduction: Reduction, fill: T) -> T {
    match reduction {
        Reduction::Average => {
            let total = values
                .iter()
                .fold(0.0f64, |total, value| total + value.to_f64());
            T::from_f64(total / values.len() as f64)
        }
        Reduction::Presence => values
            .iter()
            .copied()
            .filter(|value| value.to_f64() != fill.to_f64())
            .max_by(|left, right| left.to_f64().total_cmp(&right.to_f64()))
            .unwrap_or(fill),
        Reduction::Dominant => {
            let mut counts = [(0u32, 0.0f64); 8];
            let mut distinct = 0usize;
            for value in values {
                let fixed = value.to_f64();
                match counts[..distinct]
                    .iter_mut()
                    .find(|(_, held)| *held == fixed)
                {
                    Some((count, _)) => *count += 1,
                    None => {
                        counts[distinct] = (1, fixed);
                        distinct += 1;
                    }
                }
            }
            counts[..distinct]
                .iter()
                .filter(|(_, value)| *value != fill.to_f64())
                .max_by(|left, right| left.0.cmp(&right.0).then(right.1.total_cmp(&left.1)))
                .map_or(fill, |(_, value)| T::from_f64(*value))
        }
    }
}
