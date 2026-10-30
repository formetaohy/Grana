#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Intensity {
    min: f32,
    max: f32,
}

impl Intensity {
    pub fn new(min: f32, max: f32) -> Self {
        assert!(
            min.is_finite() && max.is_finite() && min <= max,
            "intensity range must be finite and ordered"
        );
        Self { min, max }
    }

    pub fn min(self) -> f32 {
        self.min
    }

    pub fn max(self) -> f32 {
        self.max
    }

    pub fn span(self) -> f32 {
        self.max - self.min
    }

    pub fn unit(self, value: f32) -> f32 {
        if self.span() > 0.0 && value.is_finite() {
            ((value - self.min) / self.span()).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    pub fn value(self, unit: f32) -> f32 {
        self.min + unit.clamp(0.0, 1.0) * self.span()
    }
}
