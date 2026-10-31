use glam::Vec3;

use crate::camera::Ray;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Crop {
    min: Vec3,
    max: Vec3,
    enabled: bool,
}

impl Crop {
    pub fn new(min: Vec3, max: Vec3) -> Self {
        assert!(min.is_finite() && max.is_finite(), "a crop box is finite");
        assert!(
            min.cmplt(max).all(),
            "a crop box holds a positive extent on every axis"
        );
        Self {
            min,
            max,
            enabled: true,
        }
    }

    pub fn full(min: Vec3, max: Vec3) -> Self {
        let mut crop = Self::new(min, max);
        crop.enabled = false;
        crop
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
        assert!(min.is_finite() && max.is_finite(), "a crop box is finite");
        assert!(
            min.cmplt(max).all(),
            "a crop box holds a positive extent on every axis"
        );
        self.min = min;
        self.max = max;
    }

    pub fn contains(self, point: Vec3) -> bool {
        !self.enabled || (point.cmpge(self.min).all() && point.cmple(self.max).all())
    }

    pub fn interval(self, ray: &Ray) -> Option<(f32, f32)> {
        if !self.enabled {
            return Some((0.0, f32::INFINITY));
        }
        let mut entry = 0.0f32;
        let mut exit = f32::INFINITY;
        for axis in 0..3 {
            let origin = ray.origin[axis];
            let direction = ray.direction[axis];
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
}
