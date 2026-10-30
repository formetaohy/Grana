#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpotParams {
    diameter: f32,
    threshold: f32,
}

impl SpotParams {
    pub fn new(diameter: f32, threshold: f32) -> Self {
        assert!(
            diameter.is_finite() && diameter > 0.0,
            "a spot diameter must be a positive length"
        );
        assert!(
            threshold.is_finite() && threshold >= 0.0,
            "a spot threshold must be a non-negative contrast"
        );
        Self {
            diameter,
            threshold,
        }
    }

    pub fn diameter(self) -> f32 {
        self.diameter
    }

    pub fn threshold(self) -> f32 {
        self.threshold
    }

    pub fn set_diameter(&mut self, diameter: f32) {
        assert!(
            diameter.is_finite() && diameter > 0.0,
            "a spot diameter must be a positive length"
        );
        self.diameter = diameter;
    }

    pub fn set_threshold(&mut self, threshold: f32) {
        assert!(
            threshold.is_finite() && threshold >= 0.0,
            "a spot threshold must be a non-negative contrast"
        );
        self.threshold = threshold;
    }
}
