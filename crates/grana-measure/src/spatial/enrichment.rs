use std::collections::BTreeMap;

use super::grid::Grid;
use super::{Census, Window};
use crate::csv;
use crate::math::normal_two_sided;
use crate::random::SplitMix64;
use crate::significance::benjamini_hochberg;

const MAXIMUM_CLASSES: usize = 64;

const SIGNIFICANCE: f64 = 0.05;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct EnrichmentPoint {
    position: [f32; 3],
    class: u32,
}

impl EnrichmentPoint {
    pub(crate) fn new(position: [f32; 3], class: u32) -> Self {
        assert!(
            position.iter().all(|value| value.is_finite()),
            "a point of a neighbourhood enrichment lies at finite coordinates"
        );
        Self { position, class }
    }
}

pub(crate) type Labelled = (u32, Vec<EnrichmentPoint>);

#[derive(Clone, Debug, PartialEq)]
pub struct EnrichmentClass {
    code: u32,
    index_cells: u64,
}

impl EnrichmentClass {
    pub fn code(&self) -> u32 {
        self.code
    }

    pub fn index_cells(&self) -> u64 {
        self.index_cells
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EnrichmentPair {
    index: usize,
    neighbour: usize,
    observed: f64,
    mean: f64,
    deviation: f64,
    z: f64,
    p: f64,
    q: f64,
    ccr: f64,
    significant: bool,
}

impl EnrichmentPair {
    pub fn index(&self) -> usize {
        self.index
    }

    pub fn neighbour(&self) -> usize {
        self.neighbour
    }

    pub fn observed(&self) -> f64 {
        self.observed
    }

    pub fn mean(&self) -> f64 {
        self.mean
    }

    pub fn deviation(&self) -> f64 {
        self.deviation
    }

    pub fn z(&self) -> f64 {
        self.z
    }

    pub fn p(&self) -> f64 {
        self.p
    }

    pub fn q(&self) -> f64 {
        self.q
    }

    pub fn ccr(&self) -> f64 {
        self.ccr
    }

    pub fn significant(&self) -> bool {
        self.significant
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Enrichment {
    classes: Vec<EnrichmentClass>,
    pairs: Vec<EnrichmentPair>,
    radius: f32,
    permutations: u32,
    census: Census,
}

impl Enrichment {
    pub fn classes(&self) -> &[EnrichmentClass] {
        &self.classes
    }

    pub fn pairs(&self) -> &[EnrichmentPair] {
        &self.pairs
    }

    pub fn pair(&self, index: usize, neighbour: usize) -> &EnrichmentPair {
        assert!(
            index < self.classes.len() && neighbour < self.classes.len(),
            "a neighbourhood pair names two classes of its matrix"
        );
        &self.pairs[index * self.classes.len() + neighbour]
    }

    pub fn radius(&self) -> f32 {
        self.radius
    }

    pub fn permutations(&self) -> u32 {
        self.permutations
    }

    pub fn census(&self) -> Census {
        self.census
    }

    pub fn significant(&self) -> usize {
        self.pairs.iter().filter(|pair| pair.significant).count()
    }

    pub fn largest(&self) -> f64 {
        self.pairs
            .iter()
            .map(|pair| pair.z.abs())
            .fold(0.0, f64::max)
    }

    pub fn table(&self) -> String {
        let mut text = csv::line(&[
            "class_i",
            "class_j",
            "s_obs",
            "mean",
            "sd",
            "z",
            "q",
            "ccr",
            "significant",
        ]);
        for pair in &self.pairs {
            text.push_str(&csv::line(&[
                &self.classes[pair.index].code.to_string(),
                &self.classes[pair.neighbour].code.to_string(),
                &format!("{:.4}", pair.observed),
                &format!("{:.4}", pair.mean),
                &format!("{:.4}", pair.deviation),
                &format!("{:.3}", pair.z),
                &csv::scientific(pair.q),
                &format!("{:.4}", pair.ccr),
                if pair.significant { "1" } else { "0" },
            ]));
        }
        text
    }
}

struct Neighbourhood {
    labels: Vec<u32>,
    eligible: Vec<usize>,
    offsets: Vec<usize>,
    neighbours: Vec<usize>,
}

pub(crate) fn enrichment(
    frames: &[Labelled],
    window: Window,
    radius: f32,
    permutations: u32,
    seed: u64,
) -> Option<Enrichment> {
    assert!(
        radius.is_finite() && radius > 0.0,
        "a neighbourhood enrichment names a positive radius"
    );
    assert!(
        permutations > 0,
        "a neighbourhood enrichment permutes its labels at least once"
    );
    let inside = frames
        .iter()
        .map(|(time, points)| {
            (
                *time,
                points
                    .iter()
                    .copied()
                    .filter(|point| window.contains(point.position))
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<Vec<_>>();
    let mut codes = BTreeMap::<u32, usize>::new();
    for (_, points) in &inside {
        for point in points {
            codes.entry(point.class).or_insert(0);
        }
    }
    if !(2..=MAXIMUM_CLASSES).contains(&codes.len()) {
        return None;
    }
    let codes = codes.into_keys().collect::<Vec<_>>();
    let mut lookup = BTreeMap::<u32, usize>::new();
    for (slot, code) in codes.iter().enumerate() {
        lookup.insert(*code, slot);
    }
    let mut classes = codes
        .iter()
        .map(|code| EnrichmentClass {
            code: *code,
            index_cells: 0,
        })
        .collect::<Vec<_>>();
    let count = classes.len();
    let mut census = Census {
        frames: inside.len() as u32,
        objects: inside.iter().map(|(_, points)| points.len() as u64).sum(),
        ..Census::default()
    };
    let mut observed_edges = vec![0u64; count * count];
    let mut observed_contributors = vec![0u64; count * count];
    let mut neighbourhoods = Vec::with_capacity(inside.len());
    for (_, points) in &inside {
        if points.len() < 2 {
            census.single += 1;
            continue;
        }
        let positions = points
            .iter()
            .map(|point| point.position)
            .collect::<Vec<_>>();
        let labels = points
            .iter()
            .map(|point| lookup[&point.class] as u32)
            .collect::<Vec<_>>();
        let grid = Grid::new(&positions, radius);
        let mut eligible = Vec::new();
        let mut offsets = vec![0usize];
        let mut neighbours = Vec::new();
        for (index, point) in points.iter().enumerate() {
            if window.clearance(point.position) < radius {
                continue;
            }
            eligible.push(index);
            grid.within(point.position, radius, |other, _| {
                if other != index {
                    neighbours.push(other);
                }
            });
            offsets.push(neighbours.len());
        }
        if eligible.is_empty() {
            census.single += 1;
            continue;
        }
        census.valid += 1;
        census.sampled += eligible.len() as u64;
        census.partners += neighbours.len() as u64;
        let mut counts = vec![0u32; count];
        for (slot, &index) in eligible.iter().enumerate() {
            let start = offsets[slot];
            let end = offsets[slot + 1];
            let left = labels[index] as usize;
            classes[left].index_cells += 1;
            for &other in &neighbours[start..end] {
                let right = labels[other] as usize;
                counts[right] += 1;
                observed_edges[left * count + right] += 1;
            }
            for (right, held) in counts.iter_mut().enumerate() {
                if *held > 0 {
                    observed_contributors[left * count + right] += 1;
                    *held = 0;
                }
            }
        }
        neighbourhoods.push(Neighbourhood {
            labels,
            eligible,
            offsets,
            neighbours,
        });
    }
    if census.valid == 0 {
        return None;
    }
    let mut observed = vec![0.0f64; count * count];
    let mut ccr = vec![0.0f64; count * count];
    for (left, class) in classes.iter().enumerate() {
        for right in 0..count {
            let pair = left * count + right;
            observed[pair] =
                observed_edges[pair] as f64 / observed_contributors[pair].max(1) as f64;
            ccr[pair] = if class.index_cells == 0 {
                0.0
            } else {
                observed_contributors[pair] as f64 / class.index_cells as f64
            };
        }
    }
    let mut rng = SplitMix64::new(seed);
    let mut sums = vec![0.0f64; count * count];
    let mut squares = vec![0.0f64; count * count];
    let mut edges = vec![0u64; count * count];
    let mut contributors = vec![0u64; count * count];
    let mut counts = vec![0u32; count];
    let mut labels = Vec::<u32>::new();
    for _ in 0..permutations {
        edges.fill(0);
        contributors.fill(0);
        for neighbourhood in &neighbourhoods {
            labels.clear();
            labels.extend_from_slice(&neighbourhood.labels);
            shuffle(&mut labels, &mut rng);
            for (slot, &index) in neighbourhood.eligible.iter().enumerate() {
                let start = neighbourhood.offsets[slot];
                let end = neighbourhood.offsets[slot + 1];
                let left = labels[index] as usize;
                for &other in &neighbourhood.neighbours[start..end] {
                    counts[labels[other] as usize] += 1;
                }
                for (right, held) in counts.iter_mut().enumerate() {
                    if *held > 0 {
                        edges[left * count + right] += u64::from(*held);
                        contributors[left * count + right] += 1;
                        *held = 0;
                    }
                }
            }
        }
        for pair in 0..count * count {
            let value = edges[pair] as f64 / contributors[pair].max(1) as f64;
            sums[pair] += value;
            squares[pair] += value * value;
        }
    }
    let total = f64::from(permutations);
    let mut p = vec![0.0f64; count * count];
    let mut pairs = Vec::with_capacity(count * count);
    for index in 0..count {
        for neighbour in 0..count {
            let pair = index * count + neighbour;
            let mean = sums[pair] / total;
            let variance = (squares[pair] / total - mean * mean).max(0.0);
            let deviation = variance.sqrt();
            let z = if deviation > 0.0 {
                (observed[pair] - mean) / deviation
            } else {
                0.0
            };
            p[pair] = normal_two_sided(z);
            pairs.push(EnrichmentPair {
                index,
                neighbour,
                observed: observed[pair],
                mean,
                deviation,
                z,
                p: 0.0,
                q: 0.0,
                ccr: ccr[pair],
                significant: false,
            });
        }
    }
    let corrected = benjamini_hochberg(&p);
    for ((pair, raw), adjusted) in pairs.iter_mut().zip(p).zip(corrected) {
        pair.p = raw;
        pair.q = adjusted;
        pair.significant = adjusted < SIGNIFICANCE;
    }
    Some(Enrichment {
        classes,
        pairs,
        radius,
        permutations,
        census,
    })
}

fn shuffle(labels: &mut [u32], rng: &mut SplitMix64) {
    for index in (1..labels.len()).rev() {
        let other = rng.below(index + 1);
        labels.swap(index, other);
    }
}
