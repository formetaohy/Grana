use glam::Vec3;

use crate::brick::BrickTable;
use crate::shadow::ShadowSpace;

pub(crate) const REGION_LIMIT: usize = 64;

pub(crate) fn regions(
    table: &BrickTable,
    indices: &[u32],
    space: &ShadowSpace,
    dims: [u32; 3],
) -> Vec<[u32; 4]> {
    let mut rects = indices
        .iter()
        .filter_map(|index| brick(table, *index as usize, space, dims))
        .collect::<Vec<_>>();
    merge(&mut rects);
    rects
}

fn brick(
    table: &BrickTable,
    index: usize,
    space: &ShadowSpace,
    dims: [u32; 3],
) -> Option<[u32; 4]> {
    let coord = table.grid.coord(index);
    let origin = table.grid.origin(coord);
    let extent = table.grid.extent_of(coord);
    let low = Vec3::from(origin.map(|value| value as f32));
    let high = low + Vec3::new(extent.x() as f32, extent.y() as f32, extent.z() as f32);
    let view = space.view();
    let mut bounds = [[f32::INFINITY, f32::NEG_INFINITY]; 3];
    for corner in 0..8 {
        let voxel = Vec3::new(
            if corner & 1 == 0 { low.x } else { high.x },
            if corner & 2 == 0 { low.y } else { high.y },
            if corner & 4 == 0 { low.z } else { high.z },
        );
        let world = table.origin
            + table.axes[0] * voxel.x
            + table.axes[1] * voxel.y
            + table.axes[2] * voxel.z;
        let light = view * world.extend(1.0);
        for axis in 0..3 {
            bounds[axis][0] = bounds[axis][0].min(light[axis]);
            bounds[axis][1] = bounds[axis][1].max(light[axis]);
        }
    }
    if bounds[2][1] < 0.0 || bounds[2][0] > 1.0 {
        return None;
    }
    let span = dims.map(|value| value as f32);
    let range = |axis: usize| {
        let first = bounds[axis][0] * span[axis] - 1.0;
        let last = bounds[axis][1] * span[axis] + 1.0;
        let low = first.ceil().clamp(0.0, span[axis]) as u32;
        let high = last.floor().clamp(0.0, span[axis]) as u32;
        (low, high)
    };
    let (x0, x1) = range(0);
    let (y0, y1) = range(1);
    if x0 >= x1 || y0 >= y1 {
        return None;
    }
    Some([x0, y0, x1, y1])
}

fn merge(rects: &mut Vec<[u32; 4]>) {
    rects.sort_unstable();
    let mut merged: Vec<[u32; 4]> = Vec::with_capacity(rects.len());
    for rect in rects.drain(..) {
        if let Some(held) = merged.last_mut()
            && overlaps(*held, rect)
        {
            *held = [
                held[0].min(rect[0]),
                held[1].min(rect[1]),
                held[2].max(rect[2]),
                held[3].max(rect[3]),
            ];
            continue;
        }
        merged.push(rect);
    }
    *rects = merged;
}

fn overlaps(left: [u32; 4], right: [u32; 4]) -> bool {
    left[0] < right[2] && right[0] < left[2] && left[1] < right[3] && right[1] < left[3]
}
