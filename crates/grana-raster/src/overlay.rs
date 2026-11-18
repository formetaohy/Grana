use crate::Bitmap;

#[derive(Clone, Debug, PartialEq)]
pub struct Overlay {
    bitmap: Bitmap,
    corners: [[f32; 3]; 4],
    tint: [f32; 3],
    opacity: f32,
}

impl Overlay {
    pub fn new(bitmap: Bitmap, corners: [[f32; 3]; 4], tint: [f32; 3], opacity: f32) -> Self {
        assert!(
            corners.iter().flatten().all(|value| value.is_finite()),
            "an overlay image lies at finite corners"
        );
        assert!(
            tint.iter().all(|value| (0.0..=1.0).contains(value)),
            "an overlay tint is a linear rgb triple"
        );
        assert!(
            opacity.is_finite() && (0.0..=1.0).contains(&opacity),
            "an overlay fades by a bounded opacity"
        );
        Self {
            bitmap,
            corners,
            tint,
            opacity,
        }
    }

    pub fn bitmap(&self) -> &Bitmap {
        &self.bitmap
    }

    pub fn corners(&self) -> [[f32; 3]; 4] {
        self.corners
    }

    pub fn tint(&self) -> [f32; 3] {
        self.tint
    }

    pub fn opacity(&self) -> f32 {
        self.opacity
    }
}
