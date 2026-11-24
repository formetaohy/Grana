use crate::{NoData, VoxelValue};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    Nearest,
    Linear,
}

pub fn resample<T: VoxelValue>(
    data: &[T],
    dims: [usize; 3],
    position: [f64; 3],
    filter: Filter,
    no_data: Option<NoData>,
) -> Option<f32> {
    assert_eq!(
        data.len(),
        dims[0] * dims[1] * dims[2],
        "the samples fill the named extent"
    );
    match filter {
        Filter::Nearest => nearest(data, dims, position, no_data),
        Filter::Linear => linear(data, dims, position, no_data),
    }
}

pub fn nearest_index(dims: [usize; 3], position: [f64; 3]) -> Option<usize> {
    let at = [
        position[0].round() as i64,
        position[1].round() as i64,
        position[2].round() as i64,
    ];
    offset(dims, at)
}

fn nearest<T: VoxelValue>(
    data: &[T],
    dims: [usize; 3],
    position: [f64; 3],
    no_data: Option<NoData>,
) -> Option<f32> {
    let at = nearest_index(dims, position)?;
    if absent(data[at], no_data) {
        return None;
    }
    Some(data[at].to_f32())
}

fn linear<T: VoxelValue>(
    data: &[T],
    dims: [usize; 3],
    position: [f64; 3],
    no_data: Option<NoData>,
) -> Option<f32> {
    let low = [
        position[0].floor(),
        position[1].floor(),
        position[2].floor(),
    ];
    let weight = [
        position[0] - low[0],
        position[1] - low[1],
        position[2] - low[2],
    ];
    let mut sum = 0.0f64;
    let mut held = 0.0f64;
    for corner in 0..8usize {
        let mut factor = 1.0f64;
        for (axis, component) in weight.iter().enumerate() {
            factor *= if corner & (1 << axis) == 0 {
                1.0 - *component
            } else {
                *component
            };
        }
        if factor == 0.0 {
            continue;
        }
        let at = [
            low[0] as i64 + (corner & 1) as i64,
            low[1] as i64 + ((corner >> 1) & 1) as i64,
            low[2] as i64 + ((corner >> 2) & 1) as i64,
        ];
        let Some(index) = offset(dims, at) else {
            continue;
        };
        if absent(data[index], no_data) {
            continue;
        }
        sum += data[index].to_f64() * factor;
        held += factor;
    }
    (held > 0.0).then(|| (sum / held) as f32)
}

fn absent<T: VoxelValue>(value: T, no_data: Option<NoData>) -> bool {
    no_data.is_some_and(|no_data| value.is_no_data(no_data))
}

fn offset(dims: [usize; 3], at: [i64; 3]) -> Option<usize> {
    if (0..3).any(|axis| at[axis] < 0 || at[axis] >= dims[axis] as i64) {
        return None;
    }
    Some(at[0] as usize + dims[0] * (at[1] as usize + dims[1] * at[2] as usize))
}
