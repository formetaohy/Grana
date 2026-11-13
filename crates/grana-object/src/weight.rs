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
        assert!(total > 0, "a spot weight samples at least one spot");
        let stride = (total / SAMPLE).max(1);
        let mut photons = Vec::with_capacity(total.div_ceil(stride).min(SAMPLE) + 1);
        let mut precisions = Vec::with_capacity(total.div_ceil(stride).min(SAMPLE) + 1);
        let mut seen = 0usize;
        for spot in spots {
            if seen.is_multiple_of(stride) {
                let quality = spot.quality();
                if quality.is_finite() && quality > 0.0 {
                    photons.push(quality);
                }
                precisions.push(lateral(spot));
            }
            seen += 1;
        }
        assert_eq!(
            seen, total,
            "a spot weight samples the count it is told it samples"
        );
        photons.sort_by(f32::total_cmp);
        precisions.sort_by(f32::total_cmp);
        let median = |values: &[f32], fallback: f32| {
            values.get(values.len() / 2).copied().unwrap_or(fallback)
        };
        Self::new(
            median(&photons, 0.0),
            median(&precisions, PRECISION_FLOOR).max(PRECISION_FLOOR),
        )
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
