use glam::Vec3;

const UNIT_TOLERANCE: f32 = 1.0e-3;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Clip {
    normal: Vec3,
    offset: f32,
}

impl Default for Clip {
    fn default() -> Self {
        Self::none()
    }
}

impl Clip {
    pub fn new(normal: Vec3, offset: f32) -> Self {
        assert!(normal.is_finite(), "a clip plane names a finite direction");
        assert!(
            normal.length_squared() > 1.0e-12,
            "a clip plane holds a direction"
        );
        assert!(offset.is_finite(), "a clip plane sits at a finite depth");
        Self {
            normal: normal.normalize(),
            offset,
        }
    }

    pub fn none() -> Self {
        Self {
            normal: Vec3::ZERO,
            offset: 0.0,
        }
    }

    pub fn restore(normal: [f32; 3], offset: f32) -> Option<Self> {
        let normal = Vec3::from(normal);
        if !normal.is_finite() || !offset.is_finite() {
            return None;
        }
        if normal == Vec3::ZERO {
            return (offset == 0.0).then_some(Self::none());
        }
        let length = normal.length();
        ((length - 1.0).abs() <= UNIT_TOLERANCE).then_some(Self { normal, offset })
    }

    pub fn enabled(self) -> bool {
        self.normal != Vec3::ZERO
    }

    pub fn normal(self) -> Vec3 {
        self.normal
    }

    pub fn offset(self) -> f32 {
        self.offset
    }

    pub fn parts(self) -> ([f32; 3], f32) {
        (self.normal.to_array(), self.offset)
    }

    pub fn flipped(self) -> Self {
        if !self.enabled() {
            return self;
        }
        Self {
            normal: -self.normal,
            offset: -self.offset,
        }
    }
}
