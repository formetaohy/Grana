use crate::Extent;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spacing {
    x: f32,
    y: f32,
    z: f32,
}

impl Spacing {
    pub fn new(x: f32, y: f32, z: f32) -> Self {
        for value in [x, y, z] {
            assert!(
                value.is_finite() && value > 0.0,
                "spacing must be finite and positive"
            );
        }
        Self { x, y, z }
    }

    pub fn x(self) -> f32 {
        self.x
    }

    pub fn y(self) -> f32 {
        self.y
    }

    pub fn z(self) -> f32 {
        self.z
    }

    pub fn world_extent(self, extent: Extent) -> [f32; 3] {
        [
            extent.x() as f32 * self.x,
            extent.y() as f32 * self.y,
            extent.z() as f32 * self.z,
        ]
    }
}
