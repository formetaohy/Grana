#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpotParams {
    diameter: f32,
    threshold: f32,
}

impl SpotParams {
    pub const FIELD_COUNT: usize = 2;

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

    pub fn field(&self, index: usize) -> f32 {
        match index {
            0 => self.diameter,
            1 => self.threshold,
            _ => panic!("a spot carries two parameters"),
        }
    }

    pub fn set_field(&mut self, index: usize, value: f32) {
        match index {
            0 => self.set_diameter(value),
            1 => self.set_threshold(value),
            _ => panic!("a spot carries two parameters"),
        }
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
    grain: f32,
    seed: f32,
    min_volume: f32,
}

impl SurfaceParams {
    pub const FIELD_COUNT: usize = 4;

    pub fn new(threshold: f32, grain: f32, seed: f32, min_volume: f32) -> Self {
        assert!(
            (0.0..=1.0).contains(&threshold),
            "a surface threshold is a fraction of the channel intensity range"
        );
        assert!(
            grain.is_finite() && grain >= 0.0,
            "a surface grain is a non-negative length"
        );
        assert!(
            seed.is_finite() && seed >= 0.0,
            "a surface seed is a non-negative length"
        );
        assert!(
            min_volume.is_finite() && min_volume >= 0.0,
            "a surface volume filter must be a non-negative volume"
        );
        Self {
            threshold,
            grain,
            seed,
            min_volume,
        }
    }

    pub fn threshold(self) -> f32 {
        self.threshold
    }

    pub fn grain(self) -> f32 {
        self.grain
    }

    pub fn seed(self) -> f32 {
        self.seed
    }

    pub fn min_volume(self) -> f32 {
        self.min_volume
    }

    pub fn field(&self, index: usize) -> f32 {
        match index {
            0 => self.threshold,
            1 => self.grain,
            2 => self.seed,
            3 => self.min_volume,
            _ => panic!("a surface carries four parameters"),
        }
    }

    pub fn set_field(&mut self, index: usize, value: f32) {
        match index {
            0 => self.set_threshold(value),
            1 => self.set_grain(value),
            2 => self.set_seed(value),
            3 => self.set_min_volume(value),
            _ => panic!("a surface carries four parameters"),
        }
    }

    pub fn set_threshold(&mut self, threshold: f32) {
        assert!(
            (0.0..=1.0).contains(&threshold),
            "a surface threshold is a fraction of the channel intensity range"
        );
        self.threshold = threshold;
    }

    pub fn set_grain(&mut self, grain: f32) {
        assert!(
            grain.is_finite() && grain >= 0.0,
            "a surface grain is a non-negative length"
        );
        self.grain = grain;
    }

    pub fn set_seed(&mut self, seed: f32) {
        assert!(
            seed.is_finite() && seed >= 0.0,
            "a surface seed is a non-negative length"
        );
        self.seed = seed;
    }

    pub fn set_min_volume(&mut self, min_volume: f32) {
        assert!(
            min_volume.is_finite() && min_volume >= 0.0,
            "a surface volume filter must be a non-negative volume"
        );
        self.min_volume = min_volume;
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FilamentParams {
    threshold: f32,
    min_length: f32,
}

impl FilamentParams {
    pub const FIELD_COUNT: usize = 2;

    pub fn new(threshold: f32, min_length: f32) -> Self {
        assert!(
            (0.0..=1.0).contains(&threshold),
            "a filament threshold is a fraction of the channel intensity range"
        );
        assert!(
            min_length.is_finite() && min_length > 0.0,
            "a filament length filter must be a positive length"
        );
        Self {
            threshold,
            min_length,
        }
    }

    pub fn threshold(self) -> f32 {
        self.threshold
    }

    pub fn min_length(self) -> f32 {
        self.min_length
    }

    pub fn field(&self, index: usize) -> f32 {
        match index {
            0 => self.threshold,
            1 => self.min_length,
            _ => panic!("a filament carries two parameters"),
        }
    }

    pub fn set_field(&mut self, index: usize, value: f32) {
        match index {
            0 => self.set_threshold(value),
            1 => self.set_min_length(value),
            _ => panic!("a filament carries two parameters"),
        }
    }

    pub fn set_threshold(&mut self, threshold: f32) {
        assert!(
            (0.0..=1.0).contains(&threshold),
            "a filament threshold is a fraction of the channel intensity range"
        );
        self.threshold = threshold;
    }

    pub fn set_min_length(&mut self, min_length: f32) {
        assert!(
            min_length.is_finite() && min_length > 0.0,
            "a filament length filter must be a positive length"
        );
        self.min_length = min_length;
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrackParams {
    max_step: f32,
    max_gap: u32,
    division: bool,
    division_range: f32,
}

impl TrackParams {
    pub fn new(max_step: f32, max_gap: u32, division: bool, division_range: f32) -> Self {
        assert!(
            max_step.is_finite() && max_step > 0.0,
            "a tracking step must be a positive length"
        );
        assert!(
            division_range.is_finite() && division_range > 0.0,
            "a division range must be a positive length"
        );
        assert!(
            !division || division_range >= max_step,
            "the division range covers the tracking step"
        );
        Self {
            max_step,
            max_gap,
            division,
            division_range,
        }
    }

    pub fn max_step(self) -> f32 {
        self.max_step
    }

    pub fn max_gap(self) -> u32 {
        self.max_gap
    }

    pub fn division(self) -> bool {
        self.division
    }

    pub fn division_range(self) -> f32 {
        self.division_range
    }
}
