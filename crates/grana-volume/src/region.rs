use std::ops::Range;

use glam::Vec3;

use crate::Level;

const VOXEL_EPSILON: f32 = 1.0e-4;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Region {
    min: Vec3,
    max: Vec3,
    enabled: bool,
}

impl Region {
    pub fn new(min: Vec3, max: Vec3) -> Self {
        assert!(min.is_finite() && max.is_finite(), "a region is finite");
        assert!(
            min.cmplt(max).all(),
            "a region holds a positive extent on every axis"
        );
        Self {
            min,
            max,
            enabled: true,
        }
    }

    pub fn full(min: Vec3, max: Vec3) -> Self {
        let mut region = Self::new(min, max);
        region.enabled = false;
        region
    }

    pub fn none() -> Self {
        Self {
            min: Vec3::splat(-1.0),
            max: Vec3::splat(1.0),
            enabled: false,
        }
    }

    pub fn enabled(self) -> bool {
        self.enabled
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    pub fn min(self) -> Vec3 {
        self.min
    }

    pub fn max(self) -> Vec3 {
        self.max
    }

    pub fn size(self) -> Vec3 {
        self.max - self.min
    }

    pub fn set_bounds(&mut self, min: Vec3, max: Vec3) {
        assert!(min.is_finite() && max.is_finite(), "a region is finite");
        assert!(
            min.cmplt(max).all(),
            "a region holds a positive extent on every axis"
        );
        self.min = min;
        self.max = max;
    }

    pub fn contains(self, point: Vec3) -> bool {
        !self.enabled || (point.cmpge(self.min).all() && point.cmple(self.max).all())
    }

    pub fn interval(self, origin: Vec3, direction: Vec3) -> Option<(f32, f32)> {
        if !self.enabled {
            return Some((0.0, f32::INFINITY));
        }
        let mut entry = 0.0f32;
        let mut exit = f32::INFINITY;
        for axis in 0..3 {
            let origin = origin[axis];
            let direction = direction[axis];
            if direction.abs() < 1.0e-9 {
                if origin < self.min[axis] || origin > self.max[axis] {
                    return None;
                }
                continue;
            }
            let inverse = 1.0 / direction;
            let near = (self.min[axis] - origin) * inverse;
            let far = (self.max[axis] - origin) * inverse;
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
        let spacing = level.spacing();
        let step = [spacing.x(), spacing.y(), spacing.z()];
        let world = level.world_extent();
        let mut ranges = [0..0, 0..0, 0..0];
        for axis in 0..3 {
            let offset = 0.5 * (world[axis] - step[axis]);
            let first = (self.min[axis] + offset) / step[axis];
            let last = (self.max[axis] + offset) / step[axis];
            let start = (first - VOXEL_EPSILON).ceil().max(0.0);
            let end = (last + VOXEL_EPSILON).floor().min((dims[axis] - 1) as f32);
            if start > end {
                return None;
            }
            ranges[axis] = start as u32..end as u32 + 1;
        }
        Some(ranges)
    }
}
