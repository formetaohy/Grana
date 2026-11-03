#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfaceCut {
    split: f32,
    min_volume: f32,
    min_sphericity: f32,
    min_mean_intensity: f32,
}

impl SurfaceCut {
    pub fn new(split: f32, min_volume: f32, min_sphericity: f32, min_mean_intensity: f32) -> Self {
        assert!(
            (0.0..=1.0).contains(&split),
            "a surface split is a fraction of the peak reach"
        );
        assert!(
            min_volume.is_finite() && min_volume >= 0.0,
            "a surface volume filter must be a non-negative volume"
        );
        assert!(
            (0.0..=1.0).contains(&min_sphericity),
            "a surface sphericity filter is a unit fraction"
        );
        assert!(
            (0.0..=1.0).contains(&min_mean_intensity),
            "a surface intensity filter is a unit fraction"
        );
        Self {
            split,
            min_volume,
            min_sphericity,
            min_mean_intensity,
        }
    }

    pub fn split(self) -> f32 {
        self.split
    }

    pub fn min_volume(self) -> f32 {
        self.min_volume
    }

    pub fn min_sphericity(self) -> f32 {
        self.min_sphericity
    }

    pub fn min_mean_intensity(self) -> f32 {
        self.min_mean_intensity
    }
}

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
pub struct SurfaceFilter {
    opening: f32,
    closing: f32,
    min_sphericity: f32,
    min_mean_intensity: f32,
}

impl SurfaceFilter {
    pub fn new(opening: f32, closing: f32, min_sphericity: f32, min_mean_intensity: f32) -> Self {
        assert!(
            opening.is_finite() && opening >= 0.0,
            "a surface opening radius is a non-negative length"
        );
        assert!(
            closing.is_finite() && closing >= 0.0,
            "a surface closing radius is a non-negative length"
        );
        assert!(
            (0.0..=1.0).contains(&min_sphericity),
            "a surface sphericity filter is a unit fraction"
        );
        assert!(
            (0.0..=1.0).contains(&min_mean_intensity),
            "a surface intensity filter is a unit fraction"
        );
        Self {
            opening,
            closing,
            min_sphericity,
            min_mean_intensity,
        }
    }

    pub fn opening(self) -> f32 {
        self.opening
    }

    pub fn closing(self) -> f32 {
        self.closing
    }

    pub fn min_sphericity(self) -> f32 {
        self.min_sphericity
    }

    pub fn min_mean_intensity(self) -> f32 {
        self.min_mean_intensity
    }

    pub fn field(self, index: usize) -> f32 {
        match index {
            0 => self.opening,
            1 => self.closing,
            2 => self.min_sphericity,
            3 => self.min_mean_intensity,
            _ => panic!("a surface filter carries four parameters"),
        }
    }

    pub fn set_field(&mut self, index: usize, value: f32) {
        match index {
            0 => self.set_opening(value),
            1 => self.set_closing(value),
            2 => self.set_min_sphericity(value),
            3 => self.set_min_mean_intensity(value),
            _ => panic!("a surface filter carries four parameters"),
        }
    }

    pub fn set_opening(&mut self, opening: f32) {
        assert!(
            opening.is_finite() && opening >= 0.0,
            "a surface opening radius is a non-negative length"
        );
        self.opening = opening;
    }

    pub fn set_closing(&mut self, closing: f32) {
        assert!(
            closing.is_finite() && closing >= 0.0,
            "a surface closing radius is a non-negative length"
        );
        self.closing = closing;
    }

    pub fn set_min_sphericity(&mut self, min_sphericity: f32) {
        assert!(
            (0.0..=1.0).contains(&min_sphericity),
            "a surface sphericity filter is a unit fraction"
        );
        self.min_sphericity = min_sphericity;
    }

    pub fn set_min_mean_intensity(&mut self, min_mean_intensity: f32) {
        assert!(
            (0.0..=1.0).contains(&min_mean_intensity),
            "a surface intensity filter is a unit fraction"
        );
        self.min_mean_intensity = min_mean_intensity;
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfaceParams {
    threshold: f32,
    grain: f32,
    split: f32,
    min_volume: f32,
    filter: SurfaceFilter,
}

impl SurfaceParams {
    pub const FIELD_COUNT: usize = 8;
    pub fn new(
        threshold: f32,
        grain: f32,
        split: f32,
        min_volume: f32,
        filter: SurfaceFilter,
    ) -> Self {
        assert!(
            (0.0..=1.0).contains(&threshold),
            "a surface threshold is a fraction of the local intensity range"
        );
        assert!(
            grain.is_finite() && grain >= 0.0,
            "a surface grain is a non-negative length"
        );
        assert!(
            (0.0..=1.0).contains(&split),
            "a surface split is a fraction of the peak reach"
        );
        assert!(
            min_volume.is_finite() && min_volume >= 0.0,
            "a surface volume filter must be a non-negative volume"
        );
        Self {
            threshold,
            grain,
            split,
            min_volume,
            filter,
        }
    }

    pub fn threshold(self) -> f32 {
        self.threshold
    }

    pub fn grain(self) -> f32 {
        self.grain
    }

    pub fn split(self) -> f32 {
        self.split
    }

    pub fn min_volume(self) -> f32 {
        self.min_volume
    }

    pub fn filter(self) -> SurfaceFilter {
        self.filter
    }

    pub fn opening(self) -> f32 {
        self.filter.opening()
    }

    pub fn closing(self) -> f32 {
        self.filter.closing()
    }

    pub fn min_sphericity(self) -> f32 {
        self.filter.min_sphericity()
    }

    pub fn min_mean_intensity(self) -> f32 {
        self.filter.min_mean_intensity()
    }

    pub fn cut(self) -> SurfaceCut {
        SurfaceCut::new(
            self.split,
            self.min_volume,
            self.filter.min_sphericity(),
            self.filter.min_mean_intensity(),
        )
    }

    pub fn field(&self, index: usize) -> f32 {
        match index {
            0 => self.threshold,
            1 => self.grain,
            2 => self.split,
            3 => self.min_volume,
            4..=7 => self.filter.field(index - 4),
            _ => panic!("a surface carries eight parameters"),
        }
    }

    pub fn set_field(&mut self, index: usize, value: f32) {
        match index {
            0 => self.set_threshold(value),
            1 => self.set_grain(value),
            2 => self.set_split(value),
            3 => self.set_min_volume(value),
            4..=7 => self.filter.set_field(index - 4, value),
            _ => panic!("a surface carries eight parameters"),
        }
    }

    pub fn set_threshold(&mut self, threshold: f32) {
        assert!(
            (0.0..=1.0).contains(&threshold),
            "a surface threshold is a fraction of the local intensity range"
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

    pub fn set_split(&mut self, split: f32) {
        assert!(
            (0.0..=1.0).contains(&split),
            "a surface split is a fraction of the peak reach"
        );
        self.split = split;
    }

    pub fn set_min_volume(&mut self, min_volume: f32) {
        assert!(
            min_volume.is_finite() && min_volume >= 0.0,
            "a surface volume filter must be a non-negative volume"
        );
        self.min_volume = min_volume;
    }

    pub fn set_filter(&mut self, filter: SurfaceFilter) {
        self.filter = filter;
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
            "a filament threshold is a fraction of the local intensity range"
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
            "a filament threshold is a fraction of the local intensity range"
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
