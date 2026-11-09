use std::ops::Range;

use glam::Vec3;

use crate::Level;

const UNIT_TOLERANCE: f32 = 1.0e-3;
const ORTHOGONAL_TOLERANCE: f32 = 1.0e-3;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Region {
    center: Vec3,
    axes: [Vec3; 3],
    half: Vec3,
    enabled: bool,
}

impl Region {
    pub fn new(center: Vec3, axes: [Vec3; 3], half: Vec3) -> Self {
        Self::checked(
            center.to_array(),
            axes.map(|axis: Vec3| axis.to_array()),
            half.to_array(),
        )
        .expect("a region names an orthonormal box of positive extent")
    }

    pub fn checked(center: [f32; 3], axes: [[f32; 3]; 3], half: [f32; 3]) -> Option<Self> {
        let center = Vec3::from(center);
        let axes = axes.map(Vec3::from);
        let half = Vec3::from(half);
        let finite = center.is_finite() && axes.iter().all(|axis| axis.is_finite());
        let unit = axes
            .iter()
            .all(|axis| (axis.length() - 1.0).abs() <= UNIT_TOLERANCE);
        let orthogonal = (0..3).all(|left| {
            (left + 1..3).all(|right| axes[left].dot(axes[right]).abs() <= ORTHOGONAL_TOLERANCE)
        });
        let positive = half.is_finite() && half.cmpgt(Vec3::ZERO).all();
        if !(finite && unit && orthogonal && positive) {
            return None;
        }
        Some(Self {
            center,
            axes,
            half,
            enabled: true,
        })
    }

    pub fn axis_aligned(min: Vec3, max: Vec3) -> Self {
        Self::new(
            Vec3::splat(0.5) * (min + max),
            [Vec3::X, Vec3::Y, Vec3::Z],
            Vec3::splat(0.5) * (max - min),
        )
    }

    pub fn full(min: Vec3, max: Vec3) -> Self {
        let mut region = Self::axis_aligned(min, max);
        region.enabled = false;
        region
    }

    pub fn none() -> Self {
        Self {
            center: Vec3::ZERO,
            axes: [Vec3::X, Vec3::Y, Vec3::Z],
            half: Vec3::splat(0.5),
            enabled: false,
        }
    }

    pub fn enabled(self) -> bool {
        self.enabled
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    pub fn center(self) -> Vec3 {
        self.center
    }

    pub fn axis(self, index: usize) -> Vec3 {
        self.axes[index]
    }

    pub fn axes(self) -> [Vec3; 3] {
        self.axes
    }

    pub fn half(self) -> Vec3 {
        self.half
    }

    pub fn size(self) -> Vec3 {
        2.0 * self.half
    }

    pub fn reach(self) -> Vec3 {
        (0..3)
            .map(|index| self.axes[index].abs() * self.half[index])
            .sum()
    }

    pub fn bounds(self) -> (Vec3, Vec3) {
        let reach = self.reach();
        (self.center - reach, self.center + reach)
    }

    pub fn corner(self, index: usize) -> Vec3 {
        let mut corner = self.center;
        for axis in 0..3 {
            let sign = if index & (1 << axis) == 0 { -1.0 } else { 1.0 };
            corner += self.axes[axis] * (sign * self.half[axis]);
        }
        corner
    }

    pub fn set_center(&mut self, center: Vec3) {
        assert!(center.is_finite(), "a region center is a finite point");
        self.center = center;
    }

    pub fn set_half(&mut self, half: Vec3) {
        assert!(
            half.is_finite() && half.cmpgt(Vec3::ZERO).all(),
            "a region holds a positive extent on every axis"
        );
        self.half = half;
    }

    pub fn contains(self, point: Vec3) -> bool {
        !self.enabled || self.local(point).abs().cmple(self.half).all()
    }

    pub fn local(self, point: Vec3) -> Vec3 {
        let offset = point - self.center;
        Vec3::new(
            offset.dot(self.axes[0]),
            offset.dot(self.axes[1]),
            offset.dot(self.axes[2]),
        )
    }

    pub fn interval(self, origin: Vec3, direction: Vec3) -> Option<(f32, f32)> {
        if !self.enabled {
            return Some((0.0, f32::INFINITY));
        }
        let local = self.local(origin);
        let mut entry = 0.0f32;
        let mut exit = f32::INFINITY;
        for axis in 0..3 {
            let origin = local[axis];
            let direction = direction.dot(self.axes[axis]);
            let half = self.half[axis];
            if direction.abs() < 1.0e-9 {
                if origin < -half || origin > half {
                    return None;
                }
                continue;
            }
            let inverse = 1.0 / direction;
            let near = (-half - origin) * inverse;
            let far = (half - origin) * inverse;
            entry = entry.max(near.min(far));
            exit = exit.min(near.max(far));
        }
        (entry <= exit).then_some((entry, exit))
    }

    pub fn voxel_range(self, level: Level) -> Option<[Range<u32>; 3]> {
        let extent = level.extent();
        let dims = [extent.x(), extent.y(), extent.z()];
        if !self.enabled {
            return Some([0..dims[0], 0..dims[1], 0..dims[2]]);
        }
        let mut low = [f32::INFINITY; 3];
        let mut high = [f32::NEG_INFINITY; 3];
        for corner in 0..8 {
            let voxel = level.voxel_of(self.corner(corner).to_array());
            for axis in 0..3 {
                low[axis] = low[axis].min(voxel[axis]);
                high[axis] = high[axis].max(voxel[axis]);
            }
        }
        let mut ranges = [0..0, 0..0, 0..0];
        for axis in 0..3 {
            let start = low[axis].floor().max(0.0);
            let end = (high[axis].ceil() - 1.0).min((dims[axis] - 1) as f32);
            if start > end {
                return None;
            }
            ranges[axis] = start as u32..end as u32 + 1;
        }
        Some(ranges)
    }
}
