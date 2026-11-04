use crate::IntensityTable;

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
    intensities: IntensityTable,
}

impl SpotSet {
    pub fn new(level: usize, spots: Vec<Spot>, intensities: IntensityTable) -> Self {
        assert_eq!(
            intensities.objects(),
            spots.len(),
            "every spot is measured in every channel"
        );
        Self {
            level,
            spots,
            intensities,
        }
    }

    pub fn level(&self) -> usize {
        self.level
    }

    pub fn spots(&self) -> &[Spot] {
        &self.spots
    }

    pub fn intensities(&self) -> &IntensityTable {
        &self.intensities
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

    pub fn without(&self, object: usize) -> Option<Self> {
        if object >= self.spots.len() {
            return None;
        }
        let mut spots = self.spots.clone();
        spots.remove(object);
        Some(Self::new(
            self.level,
            spots,
            self.intensities.without(object),
        ))
    }

    pub fn retain(&self, keep: &[bool]) -> Self {
        assert_eq!(
            keep.len(),
            self.spots.len(),
            "the kept objects cover every spot"
        );
        let mut spots = Vec::with_capacity(keep.iter().filter(|kept| **kept).count());
        for (object, kept) in keep.iter().enumerate() {
            if *kept {
                spots.push(self.spots[object]);
            }
        }
        Self::new(self.level, spots, self.intensities.retain(keep))
    }

    pub fn merge(&self, other: &Self) -> Self {
        assert_eq!(
            self.level, other.level,
            "a merged spot set holds one resolution level"
        );
        let mut spots = self.spots.clone();
        spots.extend_from_slice(&other.spots);
        Self::new(
            self.level,
            spots,
            self.intensities.merge(&other.intensities),
        )
    }
}
