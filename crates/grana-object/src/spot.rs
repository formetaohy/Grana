#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spot {
    center: [f32; 3],
    radius: f32,
    quality: f32,
}

impl Spot {
    pub fn new(center: [f32; 3], radius: f32, quality: f32) -> Self {
        assert!(
            center.iter().all(|value| value.is_finite()),
            "a spot center must be finite"
        );
        assert!(
            radius.is_finite() && radius > 0.0,
            "a spot radius must be a positive length"
        );
        assert!(
            quality.is_finite(),
            "a spot quality must be a finite contrast"
        );
        Self {
            center,
            radius,
            quality,
        }
    }

    pub fn center(self) -> [f32; 3] {
        self.center
    }

    pub fn radius(self) -> f32 {
        self.radius
    }

    pub fn quality(self) -> f32 {
        self.quality
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SpotSet {
    level: usize,
    spots: Vec<Spot>,
}

impl SpotSet {
    pub fn new(level: usize, spots: Vec<Spot>) -> Self {
        Self { level, spots }
    }

    pub fn level(&self) -> usize {
        self.level
    }

    pub fn spots(&self) -> &[Spot] {
        &self.spots
    }

    pub fn len(&self) -> usize {
        self.spots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.spots.is_empty()
    }

    pub fn mean_quality(&self) -> Option<f32> {
        if self.spots.is_empty() {
            return None;
        }
        let total: f32 = self.spots.iter().map(|spot| spot.quality).sum();
        Some(total / self.spots.len() as f32)
    }
}
