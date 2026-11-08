use grana_volume::Level;

use crate::axes::Layout;

pub(crate) fn levels(layouts: &[Layout]) -> (Vec<Level>, [f32; 3]) {
    let first = layouts.first().expect("a multiscale image holds a level");
    let world = first.spacing.world_extent(first.extent);
    let first_voxel = voxel(first);
    let center = [0, 1, 2].map(|axis| -0.5 * f64::from(world[axis]) + 0.5 * first_voxel[axis]);
    let placement = [0, 1, 2].map(|axis| (first.translation[axis] - center[axis]) as f32);
    let levels = layouts
        .iter()
        .map(|layout| {
            let voxel = voxel(layout);
            let origin = [0, 1, 2].map(|axis| {
                let center = layout.translation[axis] - f64::from(placement[axis]);
                (center - 0.5 * voxel[axis]) as f32
            });
            Level::placed(layout.extent, layout.spacing, origin)
        })
        .collect();
    (levels, placement)
}

fn voxel(layout: &Layout) -> [f64; 3] {
    let spacing = layout.spacing;
    [
        f64::from(spacing.x()),
        f64::from(spacing.y()),
        f64::from(spacing.z()),
    ]
}
