use grana_object::{LocalizationFilter, SpotWeight};

use super::depth::LocalizationDepth;
use super::group::Tally;
use super::sample::Decimator;
use super::{Groups, LocalizationSource};

const SAMPLE: usize = 100_000;

const PRECISION_FLOOR: f32 = 1.0e-7;

pub struct LocalizationSurvey {
    localizations: u64,
    bounds: Option<([f32; 2], [f32; 2])>,
    heights: Option<[f32; 2]>,
    axes: [f32; 3],
    sigma: f32,
    groups: Groups,
    weight: SpotWeight,
}

impl LocalizationSurvey {
    pub fn measure(source: &dyn LocalizationSource, filter: LocalizationFilter) -> Self {
        Self::scan(source, filter, &|_, _| true)
            .expect("a localization survey completes without a progress report")
    }

    pub(super) fn scan(
        source: &dyn LocalizationSource,
        filter: LocalizationFilter,
        progress: &dyn Fn(usize, usize) -> bool,
    ) -> Option<Self> {
        assert!(
            !source.is_empty(),
            "a localization survey reads at least one frame"
        );
        let frames = source.len();
        let mut low = [f32::INFINITY; 2];
        let mut high = [f32::NEG_INFINITY; 2];
        let mut heights = [f32::INFINITY, f32::NEG_INFINITY];
        let mut tally = Tally::new();
        let mut samples = Decimator::new(SAMPLE);
        let mut seen = 0usize;
        for index in 0..frames {
            let frame = source.frame(index, filter);
            for (at, spot) in frame.spots().iter().enumerate() {
                if !frame.visible(at) {
                    continue;
                }
                let center = spot.center();
                for (axis, value) in center[..2].iter().enumerate() {
                    low[axis] = low[axis].min(*value);
                    high[axis] = high[axis].max(*value);
                }
                heights[0] = heights[0].min(center[2]);
                heights[1] = heights[1].max(center[2]);
                let sigma = spot.sigma();
                tally.push(
                    spot.group(),
                    0.5 * (f64::from(sigma[0]) + f64::from(sigma[1])),
                    f64::from(spot.quality()),
                );
                samples.push(
                    seen,
                    Sample {
                        sigma,
                        quality: spot.quality(),
                    },
                );
                seen += 1;
            }
            if !progress(index + 1, frames) {
                return None;
            }
        }
        let held = samples.into_values();
        let mut lateral = held
            .iter()
            .map(|sample| 0.5 * (sample.sigma[0] + sample.sigma[1]))
            .collect::<Vec<_>>();
        let mut photons = held
            .iter()
            .map(|sample| sample.quality)
            .filter(|quality| quality.is_finite() && *quality > 0.0)
            .collect::<Vec<_>>();
        let axes = [0, 1, 2].map(|axis| median_axis(&held, axis));
        lateral.sort_by(f32::total_cmp);
        photons.sort_by(f32::total_cmp);
        let weight = SpotWeight::new(
            median_of(&photons, 0.0),
            median_of(&lateral, PRECISION_FLOOR).max(PRECISION_FLOOR),
        );
        Some(Self {
            localizations: seen as u64,
            bounds: (seen > 0).then_some((low, high)),
            heights: (seen > 0).then_some(heights),
            axes,
            sigma: median_of(&lateral, 0.0),
            groups: tally.finish(),
            weight,
        })
    }

    pub fn localizations(&self) -> u64 {
        self.localizations
    }

    pub fn groups(&self) -> &Groups {
        &self.groups
    }

    pub fn weight(&self) -> SpotWeight {
        self.weight
    }

    pub fn sigma(&self) -> f32 {
        self.sigma
    }

    pub(super) fn axes(&self) -> [f32; 3] {
        self.axes
    }

    pub(super) fn bounds(&self) -> Option<([f32; 2], [f32; 2])> {
        self.bounds
    }

    pub(super) fn heights(&self) -> Option<[f32; 2]> {
        self.heights
    }

    pub(super) fn depth(&self, thickness: f32) -> Result<LocalizationDepth, String> {
        let [low, high] = self.heights.unwrap_or([0.0; 2]);
        if thickness > 0.0 {
            LocalizationDepth::sliced(low, high, thickness)
        } else {
            Ok(LocalizationDepth::projection(low))
        }
    }
}

#[derive(Clone, Copy)]
struct Sample {
    sigma: [f32; 3],
    quality: f32,
}

fn median_axis(samples: &[Sample], axis: usize) -> f32 {
    let mut values = samples
        .iter()
        .map(|sample| sample.sigma[axis])
        .collect::<Vec<_>>();
    values.sort_by(f32::total_cmp);
    median_of(&values, 0.0)
}

fn median_of(values: &[f32], fallback: f32) -> f32 {
    values.get(values.len() / 2).copied().unwrap_or(fallback)
}
