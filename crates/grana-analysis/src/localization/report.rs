use std::sync::Arc;

use grana_object::{ClusterSpec, LocalizationFilter, LocalizationSpec};
use grana_volume::Region;

use super::group::Groups;

const THRESHOLD: f32 = 1.0 / 7.0;

#[derive(Clone, Debug, PartialEq)]
pub struct NeNa {
    bins: Vec<f32>,
    counts: Vec<f32>,
    fit: Vec<f32>,
    precision: f32,
}

impl NeNa {
    pub(super) fn new(bins: Vec<f32>, counts: Vec<f32>, fit: Vec<f32>, precision: f32) -> Self {
        assert_eq!(
            bins.len(),
            counts.len(),
            "a NeNA histogram labels every bin it counts"
        );
        assert_eq!(
            bins.len(),
            fit.len(),
            "a NeNA histogram fits every bin it counts"
        );
        assert!(
            precision.is_finite() && precision > 0.0,
            "a NeNA fit yields a positive localization precision"
        );
        Self {
            bins,
            counts,
            fit,
            precision,
        }
    }

    pub fn bins(&self) -> &[f32] {
        &self.bins
    }

    pub fn counts(&self) -> &[f32] {
        &self.counts
    }

    pub fn fit(&self) -> &[f32] {
        &self.fit
    }

    pub fn precision(&self) -> f32 {
        self.precision
    }

    pub fn pairs(&self) -> f64 {
        self.counts.iter().map(|count| f64::from(*count)).sum()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Frc {
    frequencies: Vec<f32>,
    curve: Vec<f32>,
    smooth: Vec<f32>,
    resolution: Option<f32>,
    scoped: bool,
}

impl Frc {
    pub const THRESHOLD: f32 = THRESHOLD;

    pub(super) fn new(
        frequencies: Vec<f32>,
        curve: Vec<f32>,
        smooth: Vec<f32>,
        resolution: Option<f32>,
        scoped: bool,
    ) -> Self {
        assert_eq!(
            frequencies.len(),
            curve.len(),
            "an FRC curve labels every ring it correlates"
        );
        assert_eq!(
            frequencies.len(),
            smooth.len(),
            "an FRC curve smooths every ring it correlates"
        );
        if let Some(resolution) = resolution {
            assert!(
                resolution.is_finite() && resolution > 0.0,
                "an FRC resolution is a positive length"
            );
        }
        Self {
            frequencies,
            curve,
            smooth,
            resolution,
            scoped,
        }
    }

    pub fn frequencies(&self) -> &[f32] {
        &self.frequencies
    }

    pub fn curve(&self) -> &[f32] {
        &self.curve
    }

    pub fn smooth(&self) -> &[f32] {
        &self.smooth
    }

    pub fn resolution(&self) -> Option<f32> {
        self.resolution
    }

    pub fn scoped(&self) -> bool {
        self.scoped
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct LocalizationImage {
    size: [u32; 2],
    pixels: Arc<[u8]>,
}

impl LocalizationImage {
    pub(super) fn new(size: [u32; 2], pixels: Vec<u8>) -> Self {
        assert!(
            size[0] > 0 && size[1] > 0,
            "a localization image covers at least one pixel"
        );
        assert_eq!(
            pixels.len(),
            size[0] as usize * size[1] as usize * 4,
            "a localization image holds four bytes per pixel"
        );
        Self {
            size,
            pixels: pixels.into(),
        }
    }

    pub fn size(&self) -> [u32; 2] {
        self.size
    }

    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    pub fn shared(&self) -> Arc<[u8]> {
        Arc::clone(&self.pixels)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LocalizationGeometry {
    origin: [f32; 2],
    pixel: f32,
    size: [u32; 2],
    blur: f32,
    border: u32,
}

impl LocalizationGeometry {
    pub const MAXIMUM: u32 = 65_535;

    pub(super) fn new(
        origin: [f32; 2],
        pixel: f32,
        size: [u32; 2],
        blur: f32,
        border: u32,
    ) -> Self {
        assert!(
            size[0] > 0 && size[1] > 0,
            "a localization geometry covers at least one pixel"
        );
        assert!(
            pixel.is_finite() && pixel > 0.0,
            "a localization geometry rasterizes at a positive pixel size"
        );
        assert!(
            origin.iter().all(|value| value.is_finite()),
            "a localization geometry starts at a finite origin"
        );
        assert!(
            blur.is_finite() && blur >= 0.0,
            "a localization geometry blurs by a non-negative width"
        );
        Self {
            origin,
            pixel,
            size,
            blur,
            border,
        }
    }

    pub fn origin(&self) -> [f32; 2] {
        self.origin
    }

    pub fn pixel(&self) -> f32 {
        self.pixel
    }

    pub fn size(&self) -> [u32; 2] {
        self.size
    }

    pub fn blur(&self) -> f32 {
        self.blur
    }

    pub fn border(&self) -> u32 {
        self.border
    }

    pub fn rows(&self) -> u64 {
        u64::from(self.size[0]) * u64::from(self.size[1])
    }

    pub fn renderable(&self) -> bool {
        self.size[0] <= Self::MAXIMUM && self.size[1] <= Self::MAXIMUM && self.rows() > 0
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct LocalizationReport {
    pub(super) spec: LocalizationSpec,
    pub(super) filter: LocalizationFilter,
    pub(super) region: Option<Region>,
    pub(super) pixel: f32,
    pub(super) origin: [f32; 2],
    pub(super) extent: [f32; 2],
    pub(super) localizations: u64,
    pub(super) geometry: LocalizationGeometry,
    pub(super) density: f32,
    pub(super) nena: Option<NeNa>,
    pub(super) frc: Frc,
    pub(super) groups: Groups,
    pub(super) suggested: ClusterSpec,
    pub(super) image: LocalizationImage,
}

impl LocalizationReport {
    pub fn spec(&self) -> LocalizationSpec {
        self.spec
    }

    pub fn filter(&self) -> LocalizationFilter {
        self.filter
    }

    pub fn region(&self) -> Option<Region> {
        self.region
    }

    pub fn pixel(&self) -> f32 {
        self.pixel
    }

    pub fn origin(&self) -> [f32; 2] {
        self.origin
    }

    pub fn extent(&self) -> [f32; 2] {
        self.extent
    }

    pub fn localizations(&self) -> u64 {
        self.localizations
    }

    pub fn geometry(&self) -> LocalizationGeometry {
        self.geometry
    }

    pub fn density(&self) -> f32 {
        self.density
    }

    pub fn nena(&self) -> Option<&NeNa> {
        self.nena.as_ref()
    }

    pub fn frc(&self) -> &Frc {
        &self.frc
    }

    pub fn groups(&self) -> &Groups {
        &self.groups
    }

    pub fn suggested_clusters(&self) -> ClusterSpec {
        self.suggested
    }

    pub fn image(&self) -> &LocalizationImage {
        &self.image
    }
}
