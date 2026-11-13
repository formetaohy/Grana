#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LocalizationFilter {
    photons: f32,
    precision: f32,
}

impl LocalizationFilter {
    pub const NONE: Self = Self {
        photons: 0.0,
        precision: 0.0,
    };
    pub const PHOTONS_MAXIMUM: f32 = 1.0e7;
    pub const PRECISION_MAXIMUM: f32 = 1.0e4;

    pub fn new(photons: f32, precision: f32) -> Self {
        assert!(
            photons.is_finite() && (0.0..=Self::PHOTONS_MAXIMUM).contains(&photons),
            "a localization filter keeps a bounded photon count"
        );
        assert!(
            precision.is_finite() && (0.0..=Self::PRECISION_MAXIMUM).contains(&precision),
            "a localization filter keeps a bounded precision"
        );
        Self { photons, precision }
    }

    pub fn photons(&self) -> f32 {
        self.photons
    }

    pub fn precision(&self) -> f32 {
        self.precision
    }

    pub fn active(&self) -> bool {
        self.photons > 0.0 || self.precision > 0.0
    }

    pub fn keeps(&self, quality: f32, sigma: [f32; 3]) -> bool {
        (self.photons <= 0.0 || quality >= self.photons)
            && (self.precision <= 0.0 || 0.5 * (sigma[0] + sigma[1]) * 1000.0 <= self.precision)
    }

    pub fn at_photons(&self, photons: f32) -> Self {
        Self::new(photons, self.precision)
    }

    pub fn at_precision(&self, precision: f32) -> Self {
        Self::new(self.photons, precision)
    }
}

impl Default for LocalizationFilter {
    fn default() -> Self {
        Self::NONE
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LocalizationSpec {
    frames: [u32; 2],
    pixel: f32,
    blur: f32,
    depth: f32,
    filter: LocalizationFilter,
}

impl LocalizationSpec {
    pub const PIXEL_MINIMUM: f32 = 1.0;
    pub const PIXEL_MAXIMUM: f32 = 200.0;
    pub const BLUR_MAXIMUM: f32 = 200.0;
    pub const DEPTH_MAXIMUM: f32 = 10_000.0;
    pub const DEFAULT_PIXEL: f32 = 10.0;
    pub const DEFAULT_BLUR: f32 = 20.0;

    pub fn new(frames: [u32; 2], pixel: f32, blur: f32) -> Self {
        assert!(
            frames[0] <= frames[1],
            "a localization spec names an ordered frame range"
        );
        assert!(
            pixel.is_finite() && pixel > 0.0,
            "a localization spec renders at a positive pixel size"
        );
        assert!(
            blur.is_finite() && blur >= 0.0,
            "a localization spec blurs by a non-negative width"
        );
        Self {
            frames,
            pixel,
            blur,
            depth: 0.0,
            filter: LocalizationFilter::NONE,
        }
    }

    pub fn cover(frames: u32) -> Self {
        assert!(frames > 0, "a localization spec covers at least one frame");
        Self::new([0, frames - 1], Self::DEFAULT_PIXEL, Self::DEFAULT_BLUR)
    }

    pub fn frames(&self) -> [u32; 2] {
        self.frames
    }

    pub fn count(&self) -> u32 {
        self.frames[1] - self.frames[0] + 1
    }

    pub fn pixel(&self) -> f32 {
        self.pixel
    }

    pub fn blur(&self) -> f32 {
        self.blur
    }

    pub fn depth(&self) -> f32 {
        self.depth
    }

    pub fn filter(&self) -> LocalizationFilter {
        self.filter
    }

    pub fn fitted(&self, frames: u32) -> Self {
        assert!(frames > 0, "a dataset holds at least one frame");
        let last = frames - 1;
        let start = self.frames[0].min(last);
        let end = self.frames[1].clamp(start, last);
        Self {
            frames: [start, end],
            ..*self
        }
    }

    pub fn placed(&self, frames: [u32; 2]) -> Self {
        Self { frames, ..*self }
    }

    pub fn at_pixel(&self, pixel: f32) -> Self {
        Self { pixel, ..*self }
    }

    pub fn at_blur(&self, blur: f32) -> Self {
        Self { blur, ..*self }
    }

    pub fn at_depth(&self, depth: f32) -> Self {
        assert!(
            depth.is_finite() && (0.0..=Self::DEPTH_MAXIMUM).contains(&depth),
            "a localization spec slices a bounded depth"
        );
        Self { depth, ..*self }
    }

    pub fn at_filter(&self, filter: LocalizationFilter) -> Self {
        Self { filter, ..*self }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClusterSpec {
    radius: f32,
    dark: u32,
    minimum: u32,
}

impl ClusterSpec {
    pub const RADIUS_MINIMUM: f32 = 0.001;
    pub const RADIUS_MAXIMUM: f32 = 10.0;
    pub const DARK_MAXIMUM: u32 = 10_000;
    pub const MINIMUM_MAXIMUM: u32 = 100_000;
    pub const DEFAULT_RADIUS: f32 = 0.05;
    pub const DEFAULT_DARK: u32 = 3;
    pub const DEFAULT_MINIMUM: u32 = 2;

    pub fn new(radius: f32, dark: u32, minimum: u32) -> Self {
        assert!(
            radius.is_finite() && radius > 0.0,
            "a molecule clusters within a positive reach"
        );
        assert!(minimum >= 1, "a molecule holds at least one localization");
        Self {
            radius,
            dark,
            minimum,
        }
    }

    pub fn radius(&self) -> f32 {
        self.radius
    }

    pub fn dark(&self) -> u32 {
        self.dark
    }

    pub fn minimum(&self) -> u32 {
        self.minimum
    }

    pub fn at_radius(&self, radius: f32) -> Self {
        Self::new(radius, self.dark, self.minimum)
    }

    pub fn at_dark(&self, dark: u32) -> Self {
        Self::new(self.radius, dark, self.minimum)
    }

    pub fn at_minimum(&self, minimum: u32) -> Self {
        Self::new(self.radius, self.dark, minimum)
    }
}

impl Default for ClusterSpec {
    fn default() -> Self {
        Self::new(
            Self::DEFAULT_RADIUS,
            Self::DEFAULT_DARK,
            Self::DEFAULT_MINIMUM,
        )
    }
}
