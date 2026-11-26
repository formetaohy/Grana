use crate::Intensity;

const BINS: usize = 4096;

#[derive(Clone)]
pub(crate) struct Histogram {
    range: Intensity,
    counts: Vec<u32>,
}

impl Histogram {
    pub(crate) fn over(range: Intensity) -> Self {
        Self {
            range,
            counts: vec![0; BINS],
        }
    }

    pub(crate) fn add(&mut self, value: f32) {
        let span = self.range.span();
        if span <= 0.0 || !value.is_finite() {
            return;
        }
        let scaled = (value - self.range.min()) / span * BINS as f32;
        let bin = (scaled.floor().clamp(0.0, (BINS - 1) as f32)) as usize;
        self.counts[bin] += 1;
    }

    pub(crate) fn merge(&mut self, other: &Self) {
        assert_eq!(
            self.range, other.range,
            "a histogram merges the range it was built over"
        );
        for (held, added) in self.counts.iter_mut().zip(&other.counts) {
            *held += *added;
        }
    }

    pub(crate) fn background(&self) -> f32 {
        let bin = self
            .counts
            .iter()
            .enumerate()
            .max_by_key(|(_, count)| **count)
            .map_or(0, |(bin, _)| bin);
        let width = self.range.span() / BINS as f32;
        (self.range.min() + (bin as f32 + 0.5) * width).clamp(self.range.min(), self.range.max())
    }
}
