use crate::drift::Drift;

#[derive(Clone, Debug, PartialEq)]
pub struct LocalizationSource {
    pixel: f32,
    width: u32,
    height: u32,
    volume: bool,
    applied: Drift,
}

impl LocalizationSource {
    pub fn new(pixel: f32, width: u32, height: u32, volume: bool, applied: Drift) -> Self {
        assert!(
            pixel.is_finite() && pixel > 0.0,
            "a localization table carries a positive camera pixel"
        );
        assert!(
            width > 0 && height > 0,
            "a localization table covers a positive camera field"
        );
        Self {
            pixel,
            width,
            height,
            volume,
            applied,
        }
    }

    pub fn pixel(&self) -> f32 {
        self.pixel
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn volume(&self) -> bool {
        self.volume
    }

    pub fn applied(&self) -> &Drift {
        &self.applied
    }

    pub(crate) fn set_applied(&mut self, applied: Drift) {
        self.applied = applied;
    }
}
