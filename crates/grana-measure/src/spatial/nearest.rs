use std::collections::HashMap;

use super::grid::Grid;
use super::{Census, MINIMUM_CELL, Timed, Window, inside, radii, sampled};
use crate::sample::Sample;

#[derive(Clone, Debug, PartialEq)]
pub struct NearestNeighbourPoint {
    radius: f32,
    frames: u64,
    value: f32,
    lower: f32,
    upper: f32,
    expected: f32,
    beyond: bool,
}

impl NearestNeighbourPoint {
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

    pub fn expected(&self) -> f32 {
        self.expected
    }

    pub fn beyond(&self) -> bool {
        self.beyond
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct NearestNeighbour {
    points: Vec<NearestNeighbourPoint>,
    radius: f32,
    census: Census,
}

impl NearestNeighbour {
    pub fn points(&self) -> &[NearestNeighbourPoint] {
        &self.points
    }

    pub fn radius(&self) -> f32 {
        self.radius
    }

    pub fn census(&self) -> Census {
        self.census
    }

    pub fn table(&self) -> String {
        let radius = crate::column::length_key("radius");
        let mut text = crate::csv::line(&[
            &radius, "g", "frames", "ci_low", "ci_high", "csr", "outside",
        ]);
        for point in &self.points {
            text.push_str(&crate::csv::line(&[
                &format!("{:.3}", point.radius()),
                &format!("{:.4}", point.value()),
                &point.frames().to_string(),
                &format!("{:.4}", point.lower()),
                &format!("{:.4}", point.upper()),
                &format!("{:.4}", point.expected()),
                if point.beyond() { "1" } else { "0" },
            ]));
        }
        text
    }
}

pub(crate) fn nearest_neighbour(
    frames: &[Timed],
    partner: Option<&[Timed]>,
    window: Window,
) -> Option<NearestNeighbour> {
    let (radii, _) = radii(window)?;
    let widest = *radii
        .last()
        .expect("a radii ladder holds its widest radius");
    let cross = partner.is_some();
    let lookup = partner.map(|frames| {
        frames
            .iter()
            .map(|(time, points)| (*time, points.as_slice()))
            .collect::<HashMap<_, _>>()
    });
    let mut census = Census::default();
    let mut samples = vec![Vec::<f64>::new(); radii.len()];
    let mut counted = Vec::<usize>::new();
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
            None => source.clone(),
        };
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
        let partners = sampled(partners);
        if partners.is_empty() {
            census.single += 1;
            continue;
        }
        let values = curve(&source, &partners, cross, window, widest, &radii);
        census.valid += 1;
        census.partners += partners.len() as u64;
        census.sampled += source.len() as u64;
        for (order, value) in values.iter().enumerate() {
            if value.is_finite() {
                samples[order].push(*value);
            }
        }
        counted.push(partners.len());
    }
    if census.objects == 0 || census.valid == 0 {
        return None;
    }
    let intensity = counted.iter().map(|count| *count as f64).sum::<f64>()
        / (census.valid as f64 * window.volume());
    let mut points = Vec::with_capacity(radii.len());
    for (order, radius) in radii.iter().enumerate() {
        let Some(sample) = Sample::new(std::mem::take(&mut samples[order])) else {
            break;
        };
        let value = sample.mean() as f32;
        let interval = sample.mean_interval(0.95);
        let (lower, upper) = interval.unwrap_or((sample.mean(), sample.mean()));
        let expected = (1.0
            - (-(4.0 / 3.0) * std::f64::consts::PI * intensity * f64::from(*radius).powi(3)).exp())
            as f32;
        let beyond = interval.is_some() && (lower as f32 > expected || (upper as f32) < expected);
        points.push(NearestNeighbourPoint {
            radius: *radius,
            frames: sample.len() as u64,
            value,
            lower: lower as f32,
            upper: upper as f32,
            expected,
            beyond,
        });
    }
    if points.is_empty() {
        return None;
    }
    Some(NearestNeighbour {
        points,
        radius: widest,
        census,
    })
}

fn curve(
    source: &[[f32; 3]],
    partners: &[[f32; 3]],
    cross: bool,
    window: Window,
    widest: f32,
    radii: &[f32],
) -> Vec<f64> {
    let count = radii.len();
    let mut result = vec![f64::NAN; count];
    let grid = Grid::new(partners, widest.max(MINIMUM_CELL));
    let mut hits = vec![0u64; count];
    let mut usable = vec![0u64; count];
    for (index, point) in source.iter().enumerate() {
        let clearance = window.clearance(*point);
        let nearest = grid.nearest(*point, widest, (!cross).then_some(index));
        for (order, radius) in radii.iter().enumerate() {
            if clearance < *radius {
                continue;
            }
            usable[order] += 1;
            if nearest.is_some_and(|distance| distance <= *radius) {
                hits[order] += 1;
            }
        }
    }
    for order in 0..count {
        if usable[order] == 0 {
            continue;
        }
        result[order] = hits[order] as f64 / usable[order] as f64;
    }
    result
}
