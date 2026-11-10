use glam::Vec3;

use crate::Level;

const ORTHOGONAL_TOLERANCE: f32 = 1.0e-4;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SliceFrame {
    right: [f32; 3],
    up: [f32; 3],
    half_width: f32,
    half_height: f32,
    step: f32,
    count: u32,
}

fn dimension(extent: crate::Extent, axis: usize) -> u32 {
    match axis {
        0 => extent.x(),
        1 => extent.y(),
        _ => extent.z(),
    }
}

impl Level {
    pub fn slice_frame(self, axis: usize) -> SliceFrame {
        assert!(axis < 3, "a slice follows one of three voxel axes");
        let basis = self.basis();
        let raw = basis.inverse().row(axis);
        let normal = raw.normalize() * if axis == 1 { -1.0 } else { 1.0 };
        let reference = match axis {
            1 | 2 => basis.column(1),
            _ => basis.column(2),
        };
        let mut up = Vec3::from(reference);
        up -= normal * up.dot(normal);
        if up.length() < ORTHOGONAL_TOLERANCE {
            up = normal.any_orthonormal_vector();
        }
        let up = up.normalize();
        let right = up.cross(normal).normalize();
        let step = 1.0 / raw.length();
        let center = Vec3::from(self.center());
        let extent = self.extent();
        let (mut half_width, mut half_height) = (0.0f32, 0.0f32);
        for corner in 0..8u32 {
            let point = Vec3::from(self.corner([
                (corner & 1) as f32 * extent.x() as f32,
                ((corner >> 1) & 1) as f32 * extent.y() as f32,
                ((corner >> 2) & 1) as f32 * extent.z() as f32,
            ])) - center;
            half_width = half_width.max(point.dot(right).abs());
            half_height = half_height.max(point.dot(up).abs());
        }
        SliceFrame {
            right: right.to_array(),
            up: up.to_array(),
            half_width,
            half_height,
            step,
            count: dimension(extent, axis),
        }
    }

    pub fn slice_center(self, axis: usize, index: u32) -> [f32; 3] {
        assert!(axis < 3, "a slice follows one of three voxel axes");
        let extent = self.extent();
        assert!(
            index < dimension(extent, axis),
            "a slice lies inside its voxel axis"
        );
        let mut point = [0.0f32; 3];
        for (along, value) in point.iter_mut().enumerate() {
            *value = if along == axis {
                index as f32 + 0.5
            } else {
                0.5 * dimension(extent, along) as f32
            };
        }
        self.corner(point)
    }

    pub fn slice_index(self, axis: usize, point: [f32; 3]) -> u32 {
        let coordinate = self.voxel_of(point)[axis].floor();
        (coordinate.max(0.0) as u32).min(dimension(self.extent(), axis) - 1)
    }
}

impl SliceFrame {
    pub fn right(self) -> [f32; 3] {
        self.right
    }

    pub fn up(self) -> [f32; 3] {
        self.up
    }

    pub fn half_width(self) -> f32 {
        self.half_width
    }

    pub fn half_height(self) -> f32 {
        self.half_height
    }

    pub fn step(self) -> f32 {
        self.step
    }

    pub fn count(self) -> u32 {
        self.count
    }
}
