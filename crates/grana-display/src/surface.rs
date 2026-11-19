use glam::Vec2;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfaceSize {
    physical: [u32; 2],
    scale: f32,
}

impl SurfaceSize {
    pub fn new(physical: [u32; 2], scale: f32) -> Option<Self> {
        assert!(
            scale.is_finite() && scale > 0.0,
            "a surface holds a positive pixel density"
        );
        (physical[0] > 0 && physical[1] > 0).then_some(Self { physical, scale })
    }

    pub fn physical(self) -> [u32; 2] {
        self.physical
    }

    pub fn scale(self) -> f32 {
        self.scale
    }

    pub fn points(self) -> Vec2 {
        Vec2::new(
            self.physical[0] as f32 / self.scale,
            self.physical[1] as f32 / self.scale,
        )
    }

    pub fn pixels(self, point: Vec2) -> Vec2 {
        point * self.scale
    }
}
