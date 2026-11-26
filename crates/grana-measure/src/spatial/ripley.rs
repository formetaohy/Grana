use std::collections::HashMap;

use super::grid::Grid;
use super::{
    Census, Envelope, MINIMUM_CELL, SIMULATION_SEED, Timed, Window, inside, radii, sampled,
    simulations, uniform,
};
use crate::random::SplitMix64;
use crate::sample::Sample;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RipleyTest {
    p: f64,
    simulations: u32,
    beyond: u32,
}

impl RipleyTest {
    pub fn p(self) -> f64 {
        self.p
    }

    pub fn simulations(self) -> u32 {
        self.simulations
    }

    pub fn beyond(self) -> u32 {
        self.beyond
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RipleyPoint {
    radius: f32,
    frames: u64,
    value: f32,
    lower: f32,
    upper: f32,
    envelope_low: Option<f32>,
    envelope_high: Option<f32>,
    beyond: bool,
}

impl RipleyPoint {
    pub fn radius(&self) -> f32 {
        self.radius
    }

    pub fn frames(&self) -> u64 {
        self.frames
    }

    pub fn value(&self) -> f32 {
        self.value
    }

    pub fn lower(&self) -> f32 {
        self.lower
    }

    pub fn upper(&self) -> f32 {
        self.upper
    }

    pub fn envelope(&self) -> Option<(f32, f32)> {
        self.envelope_low.zip(self.envelope_high)
    }

    pub fn beyond(&self) -> bool {
        self.beyond
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Ripley {
    points: Vec<RipleyPoint>,
    low: f32,
    high: f32,
    radius: f32,
    census: Census,
    test: Option<RipleyTest>,
}

impl Ripley {
    pub fn points(&self) -> &[RipleyPoint] {
        &self.points
    }

    pub fn low(&self) -> f32 {
        self.low
    }

    pub fn high(&self) -> f32 {
        self.high
    }

    pub fn radius(&self) -> f32 {
        self.radius
    }

    pub fn objects(&self) -> u64 {
        self.census.objects
    }

    pub fn census(&self) -> Census {
        self.census
    }

    pub fn test(&self) -> Option<RipleyTest> {
        self.test
    }

    pub fn simulations(&self) -> u32 {
        self.test.map_or(0, RipleyTest::simulations)
    }

    pub fn table(&self) -> String {
        let simulations = self.simulations();
        let radius = crate::column::length_key("radius");
        let p = self
            .test
            .map_or_else(String::new, |test| crate::csv::scientific(test.p()));
        let mut text = crate::csv::line(&[
            &radius,
            "l_minus_r",
            "frames",
            "ci_low",
            "ci_high",
            "csr_low",
            "csr_high",
            "outside",
            "simulations",
            "p",
        ]);
        for point in &self.points {
            let envelope = point.envelope();
            text.push_str(&crate::csv::line(&[
                &format!("{:.3}", point.radius()),
                &format!("{:.4}", point.value()),
                &point.frames().to_string(),
                &format!("{:.4}", point.lower()),
                &format!("{:.4}", point.upper()),
                &envelope.map_or_else(String::new, |(low, _)| format!("{low:.4}")),
                &envelope.map_or_else(String::new, |(_, high)| format!("{high:.4}")),
                if point.beyond() { "1" } else { "0" },
                &simulations.to_string(),
                &p,
            ]));
        }
        text
    }
}

struct Sampled {
    source: Vec<[f32; 3]>,
    partners: Option<Vec<[f32; 3]>>,
}

pub(crate) fn ripley(
    frames: &[Timed],
    partner: Option<&[Timed]>,
    window: Window,
) -> Option<Ripley> {
    let (radii, step) = radii(window)?;
    let widest = *radii
        .last()
        .expect("a radii ladder holds its widest radius");
    let volume = window.volume();
    let cross = partner.is_some();
    let lookup = partner.map(|frames| {
        frames
            .iter()
            .map(|(time, points)| (*time, points.as_slice()))
            .collect::<HashMap<_, _>>()
    });
    let mut census = Census::default();
    let mut samples = vec![Vec::<f64>::new(); radii.len()];
    let mut held = Vec::<Sampled>::new();
    for (time, points) in frames {
        census.frames += 1;
        let source = inside(points, window);
        census.objects += source.len() as u64;
        let partners = match &lookup {
            Some(lookup) => match lookup.get(time) {
                Some(points) => inside(points, window),
                None => {
                    census.single += 1;
                    continue;
                }
            },
            None => Vec::new(),
        };
        census.partners += partners.len() as u64;
        let ready = if cross {
            !source.is_empty() && !partners.is_empty()
        } else {
            source.len() >= 2
        };
        if !ready {
            census.single += 1;
            continue;
        }
        let source = sampled(source);
        let partners = cross.then(|| sampled(partners));
        census.valid += 1;
        census.sampled += source.len() as u64;
        let values = curve(&source, partners.as_deref(), window, volume, step, &radii);
        for (order, value) in values.iter().enumerate() {
            if value.is_finite() {
                samples[order].push(*value);
            }
        }
        held.push(Sampled { source, partners });
    }
    if census.objects == 0 || census.valid == 0 {
        return None;
    }
    let total = held
        .iter()
        .map(|frame| frame.source.len() as u64 + frame.partners.as_ref().map_or(0, Vec::len) as u64)
        .sum::<u64>();
    let simulations = simulations(total);
    let envelope = envelope(&held, window, volume, step, &radii, simulations);
    let mut points = Vec::with_capacity(radii.len());
    let mut observed = Vec::with_capacity(radii.len());
    let mut low = 0.0f32;
    let mut high = 0.0f32;
    for (order, radius) in radii.iter().enumerate() {
        let Some(sample) = Sample::new(std::mem::take(&mut samples[order])) else {
            break;
        };
        let (lower, upper) = sample
            .mean_interval(0.95)
            .unwrap_or((sample.mean(), sample.mean()));
        let value = sample.mean() as f32;
        let (envelope_low, envelope_high) = envelope.band(order);
        let beyond = envelope.beyond(order, f64::from(value));
        low = low
            .min((lower as f32).min(value))
            .min(envelope_low.unwrap_or(f32::INFINITY));
        high = high
            .max((upper as f32).max(value))
            .max(envelope_high.unwrap_or(f32::NEG_INFINITY));
        observed.push(f64::from(value));
        points.push(RipleyPoint {
            radius: *radius,
            frames: sample.len() as u64,
            value,
            lower: lower as f32,
            upper: upper as f32,
            envelope_low,
            envelope_high,
            beyond,
        });
    }
    if points.is_empty() {
        return None;
    }
    if !(low.is_finite() && high.is_finite()) {
        low = -widest;
        high = widest;
    } else if high <= low {
        let padding = (0.5 * low.abs()).max(0.5);
        low -= padding;
        high += padding;
    }
    let radius = points.last().map_or(widest, |point| point.radius);
    let test = envelope.test(&observed).map(|(p, simulations)| RipleyTest {
        p,
        simulations,
        beyond: points.iter().filter(|point| point.beyond).count() as u32,
    });
    Some(Ripley {
        points,
        low,
        high,
        radius,
        census,
        test,
    })
}

fn curve(
    source: &[[f32; 3]],
    partner: Option<&[[f32; 3]]>,
    window: Window,
    volume: f64,
    step: f32,
    radii: &[f32],
) -> Vec<f64> {
    let count = radii.len();
    let mut result = vec![f64::NAN; count];
    let cross = partner.is_some();
    let partners = partner.unwrap_or(source);
    if cross {
        if source.is_empty() || partners.is_empty() {
            return result;
        }
    } else if source.len() < 2 {
        return result;
    }
    let clearance = source
        .iter()
        .map(|point| window.clearance(*point))
        .collect::<Vec<_>>();
    let mut ordered = clearance.clone();
    ordered.sort_by(f32::total_cmp);
    let mut pairs = vec![0i64; count + 1];
    let grid = Grid::new(partners, radii[count - 1].max(MINIMUM_CELL));
    for (index, point) in source.iter().enumerate() {
        let reach = clearance[index].min(radii[count - 1]);
        if !reach.is_finite() || reach <= 0.0 {
            continue;
        }
        grid.within(*point, reach, |other, distance| {
            if !cross && other == index {
                return;
            }
            let start = ((distance / step).ceil() as i64 - 1).max(0);
            let end = (reach / step).floor() as i64 - 1;
            if end < start {
                return;
            }
            let end = end.min(count as i64 - 1);
            pairs[start as usize] += 1;
            pairs[end as usize + 1] -= 1;
        });
    }
    let denominator = if cross {
        partners.len() as f64
    } else {
        (source.len() as f64 - 1.0).max(1.0)
    };
    let mut counted = 0i64;
    for (order, radius) in radii.iter().enumerate() {
        counted += pairs[order];
        let interior = ordered.len() - ordered.partition_point(|value| *value < *radius);
        if interior == 0 {
            break;
        }
        let expected = volume * counted.max(0) as f64 / (interior as f64 * denominator);
        result[order] = (3.0 * expected / (4.0 * std::f64::consts::PI))
            .max(0.0)
            .cbrt()
            - f64::from(*radius);
    }
    result
}

fn envelope(
    frames: &[Sampled],
    window: Window,
    volume: f64,
    step: f32,
    radii: &[f32],
    simulations: u32,
) -> Envelope {
    let mut curves = Vec::with_capacity(simulations as usize);
    let mut rng = SplitMix64::new(SIMULATION_SEED);
    for _ in 0..simulations {
        let mut sums = vec![0.0f64; radii.len()];
        let mut counts = vec![0u32; radii.len()];
        for frame in frames {
            let source = uniform(&mut rng, window, frame.source.len());
            let partners = frame
                .partners
                .as_ref()
                .map(|partners| uniform(&mut rng, window, partners.len()));
            let values = curve(&source, partners.as_deref(), window, volume, step, radii);
            for (order, value) in values.iter().enumerate() {
                if value.is_finite() {
                    sums[order] += *value;
                    counts[order] += 1;
                }
            }
        }
        curves.push(
            (0..radii.len())
                .map(|order| (counts[order] > 0).then(|| sums[order] / f64::from(counts[order])))
                .collect::<Vec<_>>(),
        );
    }
    Envelope::of(curves, radii.len())
}
