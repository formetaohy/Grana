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

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfaceParams {
    threshold: f32,
    min_volume: f32,
}

impl SurfaceParams {
    pub fn new(threshold: f32, min_volume: f32) -> Self {
        assert!(
            threshold.is_finite(),
            "a surface threshold must be a finite intensity"
        );
        assert!(
            min_volume.is_finite() && min_volume >= 0.0,
            "a surface volume filter must be a non-negative volume"
        );
        Self {
            threshold,
            min_volume,
        }
    }

    pub fn threshold(self) -> f32 {
        self.threshold
    }

    pub fn min_volume(self) -> f32 {
        self.min_volume
    }

    pub fn set_threshold(&mut self, threshold: f32) {
        assert!(
            threshold.is_finite(),
            "a surface threshold must be a finite intensity"
        );
        self.threshold = threshold;
    }

    pub fn set_min_volume(&mut self, min_volume: f32) {
        assert!(
            min_volume.is_finite() && min_volume >= 0.0,
            "a surface volume filter must be a non-negative volume"
        );
        self.min_volume = min_volume;
    }
}
