use crate::VoxelValue;

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
) -> Option<f32> {
    assert_eq!(
        data.len(),
        dims[0] * dims[1] * dims[2],
        "the samples fill the named extent"
    );
    match filter {
        Filter::Nearest => nearest(data, dims, position),
        Filter::Linear => linear(data, dims, position),
    }
}

fn nearest<T: VoxelValue>(data: &[T], dims: [usize; 3], position: [f64; 3]) -> Option<f32> {
    let at = [
        position[0].round() as i64,
        position[1].round() as i64,
        position[2].round() as i64,
    ];
    Some(data[offset(dims, at)?].to_f32())
}

fn linear<T: VoxelValue>(data: &[T], dims: [usize; 3], position: [f64; 3]) -> Option<f32> {
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
    let mut sum = 0.0f32;
    let mut visited = false;
    for corner in 0..8usize {
        let mut factor = 1.0f32;
        for (axis, component) in weight.iter().enumerate() {
            factor *= if corner & (1 << axis) == 0 {
                1.0 - *component as f32
            } else {
                *component as f32
            };
        }
        let at = [
            low[0] as i64 + (corner & 1) as i64,
            low[1] as i64 + ((corner >> 1) & 1) as i64,
            low[2] as i64 + ((corner >> 2) & 1) as i64,
        ];
        let Some(index) = offset(dims, at) else {
            continue;
        };
        if factor == 0.0 {
            continue;
        }
        sum += data[index].to_f32() * factor;
        visited = true;
    }
    visited.then_some(sum)
}

fn offset(dims: [usize; 3], at: [i64; 3]) -> Option<usize> {
    if (0..3).any(|axis| at[axis] < 0 || at[axis] >= dims[axis] as i64) {
        return None;
    }
    Some(at[0] as usize + dims[0] * (at[1] as usize + dims[1] * at[2] as usize))
}
