use grana_volume::{NoData, VoxelData, VoxelValue};

use crate::slice::Plane;

pub(crate) fn reduce(
    lower: &Plane,
    upper: &Plane,
    width: u32,
    height: u32,
    averages: bool,
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
                            averages,
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
    averages: bool,
    fill: T,
    no_data: Option<NoData>,
) -> Vec<T> {
    let mut held = Vec::with_capacity((out_width as usize) * (out_height as usize));
    for y in 0..out_height {
        for x in 0..out_width {
            let base = (2 * x).min(width - 1);
            let line = (2 * y).min(height - 1);
            if !averages {
                held.push(lower[index(width, height, base, line)]);
                continue;
            }
            let mut total = 0.0f64;
            let mut present = 0u32;
            for slice in [lower, upper] {
                for offset in 0..2u32 {
                    let at = (line + offset).min(height - 1);
                    for step in 0..2u32 {
                        let value = slice[index(width, height, (base + step).min(width - 1), at)];
                        if no_data.is_some_and(|no_data| value.is_no_data(no_data)) {
                            continue;
                        }
                        total += value.to_f64();
                        present += 1;
                    }
                }
            }
            held.push(if present == 0 {
                fill
            } else {
                T::from_f64(total / f64::from(present))
            });
        }
    }
    held
}
