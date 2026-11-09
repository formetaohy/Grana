use glam::Vec3;

use crate::{Basis, Extent, Spacing};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Level {
    extent: Extent,
    basis: Basis,
    origin: [f32; 3],
}

impl Level {
    pub fn new(extent: Extent, spacing: Spacing) -> Self {
        Self::of(extent, Basis::diagonal(spacing))
    }

    pub fn of(extent: Extent, basis: Basis) -> Self {
        let reach = Vec3::new(extent.x() as f32, extent.y() as f32, extent.z() as f32);
        let origin = basis.matrix() * (-0.5 * reach);
        Self::placed(extent, basis, origin.to_array())
    }

    pub fn placed(extent: Extent, basis: Basis, origin: [f32; 3]) -> Self {
        assert!(
            origin.iter().all(|value| value.is_finite()),
            "a level origin is finite"
        );
        Self {
            extent,
            basis,
            origin,
        }
    }

    pub fn extent(self) -> Extent {
        self.extent
    }

    pub fn basis(self) -> Basis {
        self.basis
    }

    pub fn spacing(self) -> Spacing {
        self.basis.spacing()
    }

    pub fn origin(self) -> [f32; 3] {
        self.origin
    }

    pub fn center(self) -> [f32; 3] {
        let reach = Vec3::new(
            self.extent.x() as f32,
            self.extent.y() as f32,
            self.extent.z() as f32,
        );
        (Vec3::from(self.origin) + self.basis.matrix() * (0.5 * reach)).to_array()
    }

    pub fn bounds(self) -> ([f32; 3], [f32; 3]) {
        let (low, high) = self.basis.extent(self.extent);
        (
            (Vec3::from(self.origin) + low).to_array(),
            (Vec3::from(self.origin) + high).to_array(),
        )
    }

    pub fn world_extent(self) -> [f32; 3] {
        let (low, high) = self.bounds();
        [high[0] - low[0], high[1] - low[1], high[2] - low[2]]
    }

    pub fn voxel(self) -> [f32; 3] {
        [self.basis.step(0), self.basis.step(1), self.basis.step(2)]
    }

    pub fn voxel_volume(self) -> f64 {
        f64::from(self.basis.determinant().abs())
    }

    pub fn corner(self, index: [f32; 3]) -> [f32; 3] {
        (Vec3::from(self.origin) + self.basis.matrix() * Vec3::from(index)).to_array()
    }

    pub fn voxel_center(self, index: [f32; 3]) -> [f32; 3] {
        self.corner((Vec3::from(index) + Vec3::splat(0.5)).to_array())
    }

    pub fn cell_bounds(self, low: [u32; 3], high: [u32; 3]) -> ([f32; 3], [f32; 3]) {
        let mut minimum = [f32::INFINITY; 3];
        let mut maximum = [f32::NEG_INFINITY; 3];
        for corner in 0..8 {
            let index = [0, 1, 2].map(|axis| {
                if corner & (1 << axis) == 0 {
                    low[axis] as f32
                } else {
                    high[axis] as f32
                }
            });
            let point = self.corner(index);
            for (axis, value) in point.into_iter().enumerate() {
                minimum[axis] = minimum[axis].min(value);
                maximum[axis] = maximum[axis].max(value);
            }
        }
        (minimum, maximum)
    }

    pub fn voxel_of(self, point: [f32; 3]) -> [f32; 3] {
        (self.basis.inverse() * (Vec3::from(point) - Vec3::from(self.origin))).to_array()
    }

    pub fn interval(self, origin: [f32; 3], direction: [f32; 3]) -> Option<(f32, f32)> {
        let local_origin = self.basis.inverse() * (Vec3::from(origin) - Vec3::from(self.origin));
        let local_direction = self.basis.inverse() * Vec3::from(direction);
        let dims = Vec3::new(
            self.extent.x() as f32,
            self.extent.y() as f32,
            self.extent.z() as f32,
        );
        let mut entry = f32::NEG_INFINITY;
        let mut exit = f32::INFINITY;
        for axis in 0..3 {
            let step = local_direction[axis];
            let start = local_origin[axis];
            if step.abs() < 1.0e-9 {
                if start < 0.0 || start > dims[axis] {
                    return None;
                }
                continue;
            }
            let inverse = 1.0 / step;
            let first = -start * inverse;
            let second = (dims[axis] - start) * inverse;
            entry = entry.max(first.min(second));
            exit = exit.min(first.max(second));
        }
        (entry <= exit && exit >= 0.0).then_some((entry, exit))
    }

    pub fn coarser(self) -> Self {
        let matrix = self.basis.matrix();
        let basis = Basis::new([
            (2.0 * matrix.col(0)).to_array(),
            (2.0 * matrix.col(1)).to_array(),
            (2.0 * matrix.col(2)).to_array(),
        ]);
        Self::placed(
            Extent::new(
                self.extent.x().div_ceil(2),
                self.extent.y().div_ceil(2),
                self.extent.z().div_ceil(2),
            ),
            basis,
            self.origin,
        )
    }
}
