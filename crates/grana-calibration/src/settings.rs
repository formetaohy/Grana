use grana_image::Image;

const THRESHOLD_SIGMA: f32 = 8.0;
const SEPARATION: f32 = 1.5;
const LATERAL_PATCH: f32 = 0.6;
const AXIAL_PATCH: f32 = 1.2;
const HALF_LIMIT: u32 = 12;
const HALF_FLOOR: u32 = 2;
const BAND_DEPTH: f32 = 10.0;
const BAND_LIMIT: u32 = 8;
const BEAD_LIMIT: usize = 128;
const DETECTION_VOXELS: u64 = 32 << 20;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    channel: usize,
    time: u32,
    threshold: f32,
    separation: f32,
    half: [u32; 3],
    bands: u32,
}

impl Settings {
    pub fn new(
        channel: usize,
        time: u32,
        threshold: f32,
        separation: f32,
        half: [u32; 3],
        bands: u32,
    ) -> Self {
        assert!(
            threshold.is_finite() && (1.0..=64.0).contains(&threshold),
            "a bead threshold spans a plausible multiple of the background noise"
        );
        assert!(
            separation.is_finite() && separation > 0.0 && separation <= 1.0e3,
            "a bead separation is given in microns"
        );
        assert!(
            half.iter().all(|extent| (1..=HALF_LIMIT).contains(extent)),
            "a bead patch half extent stays within its bound"
        );
        assert!(
            (1..=BAND_LIMIT).contains(&bands),
            "a measured point spread function holds a bounded number of depth bands"
        );
        Self {
            channel,
            time,
            threshold,
            separation,
            half,
            bands,
        }
    }

    pub fn for_image(image: &Image, channel: usize, time: u32) -> Self {
        assert!(
            channel < image.pyramid().channel_count(),
            "the calibrated channel exists"
        );
        assert!(
            time < image.pyramid().frame_count(),
            "the calibrated frame exists"
        );
        let level = image.pyramid().level(0);
        let spacing = level.spacing();
        let extent = level.extent();
        let half = [
            patch_half(LATERAL_PATCH, spacing.x(), extent.x()),
            patch_half(LATERAL_PATCH, spacing.y(), extent.y()),
            patch_half(AXIAL_PATCH, spacing.z(), extent.z()),
        ];
        let depth = extent.z() as f32 * spacing.z();
        let bands = (depth / BAND_DEPTH).ceil().max(1.0) as u32;
        let bands = bands.min(BAND_LIMIT);
        Self::new(channel, time, THRESHOLD_SIGMA, SEPARATION, half, bands)
    }

    pub fn channel(&self) -> usize {
        self.channel
    }

    pub fn time(&self) -> u32 {
        self.time
    }

    pub fn threshold(&self) -> f32 {
        self.threshold
    }

    pub fn separation(&self) -> f32 {
        self.separation
    }

    pub fn half(&self) -> [u32; 3] {
        self.half
    }

    pub fn bands(&self) -> u32 {
        self.bands
    }

    pub fn bead_limit(&self) -> usize {
        BEAD_LIMIT
    }

    pub(crate) fn detection_voxels(&self) -> u64 {
        DETECTION_VOXELS
    }
}

fn patch_half(physical: f32, spacing: f32, extent: u32) -> u32 {
    let cells = (physical / spacing).ceil().max(1.0) as u32;
    let upper = HALF_LIMIT.min(extent.saturating_sub(1) / 2).max(1);
    let lower = HALF_FLOOR.min(upper);
    cells.max(lower).min(upper)
}
