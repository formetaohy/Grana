use crate::Spot;

const PRECISION_FLOOR: f32 = 1.0e-7;
const SAMPLE: usize = 65_536;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpotWeight {
    photons: f32,
    precision: f32,
}

impl SpotWeight {
    pub fn new(photons: f32, precision: f32) -> Self {
        assert!(
            photons.is_finite() && photons >= 0.0,
            "a spot weight normalizes by a non-negative photon count"
        );
        assert!(
            precision.is_finite() && precision > 0.0,
            "a spot weight normalizes by a positive precision"
        );
        Self { photons, precision }
    }

    pub fn measure<'a>(total: usize, spots: impl Iterator<Item = &'a Spot>) -> Self {
        let mut sampler = SpotWeightSampler::new(total);
        for spot in spots {
            sampler.push(&[*spot]);
        }
        sampler.finish()
    }

    pub fn of(&self, spot: &Spot) -> f32 {
        let sigma = lateral(spot).max(PRECISION_FLOOR);
        let brightness = if self.photons > 0.0 {
            (spot.quality().max(0.0) / self.photons).max(0.0)
        } else {
            1.0
        };
        brightness * (self.precision * self.precision) / (sigma * sigma)
    }
}

fn lateral(spot: &Spot) -> f32 {
    let sigma = spot.sigma();
    0.5 * (sigma[0] + sigma[1])
}

pub struct SpotWeightSampler {
    total: usize,
    stride: usize,
    seen: usize,
    photons: Vec<f32>,
    precisions: Vec<f32>,
}

impl SpotWeightSampler {
    pub fn new(total: usize) -> Self {
        assert!(total > 0, "a spot weight samples at least one spot");
        let stride = (total / SAMPLE).max(1);
        let room = total.div_ceil(stride).min(SAMPLE) + 1;
        Self {
            total,
            stride,
            seen: 0,
            photons: Vec::with_capacity(room),
            precisions: Vec::with_capacity(room),
        }
    }

    pub fn push(&mut self, spots: &[Spot]) {
        for spot in spots {
            if self.seen.is_multiple_of(self.stride) {
                let quality = spot.quality();
                if quality.is_finite() && quality > 0.0 {
                    self.photons.push(quality);
                }
                self.precisions.push(lateral(spot));
            }
            self.seen += 1;
        }
    }

    pub fn finish(self) -> SpotWeight {
        assert_eq!(
            self.seen, self.total,
            "a spot weight samples the count it is told it samples"
        );
        let mut photons = self.photons;
        let mut precisions = self.precisions;
        photons.sort_by(f32::total_cmp);
        precisions.sort_by(f32::total_cmp);
        let median = |values: &[f32], fallback: f32| {
            values.get(values.len() / 2).copied().unwrap_or(fallback)
        };
        SpotWeight::new(
            median(&photons, 0.0),
            median(&precisions, PRECISION_FLOOR).max(PRECISION_FLOOR),
        )
    }
}
