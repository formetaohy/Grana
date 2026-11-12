use std::collections::VecDeque;

use crate::Error;

const MIN_OVERLAP: i64 = 16;
const MIN_SCORE: f32 = 0.3;
const MIN_AGREEMENT: f32 = 0.4;
const REFINE: i64 = 2;
const RELAXATIONS: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Right,
    Below,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Offset {
    pub x: i64,
    pub y: i64,
}

impl Offset {
    pub fn new(x: i64, y: i64) -> Self {
        Self { x, y }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Edge {
    pub first: usize,
    pub second: usize,
    pub offset: Offset,
}

impl Edge {
    pub fn new(first: usize, second: usize, offset: Offset) -> Self {
        Self {
            first,
            second,
            offset,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Plane {
    width: usize,
    height: usize,
    values: Vec<f32>,
}

impl Plane {
    pub fn new(width: usize, height: usize, values: Vec<f32>) -> Self {
        assert!(
            width > 0 && height > 0,
            "a registration plane covers at least one pixel"
        );
        assert_eq!(
            values.len(),
            width * height,
            "a registration plane holds one value per pixel"
        );
        Self {
            width,
            height,
            values,
        }
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    pub fn values(&self) -> &[f32] {
        &self.values
    }
}

pub fn estimate(first: &Plane, second: &Plane, side: Side) -> Result<Offset, Error> {
    if (first.width, first.height) != (second.width, second.height) {
        return Err(Error::new(
            "the tiles of a mosaic share the extent of their registration plane",
        ));
    }
    let left = profile(first, true);
    let right = profile(second, true);
    let top = profile(first, false);
    let bottom = profile(second, false);
    let (x, y) = match side {
        Side::Right => (
            overlap(&left, &right, first.width)?,
            jitter(&top, &bottom, first.height)?,
        ),
        Side::Below => (
            jitter(&left, &right, first.width)?,
            overlap(&top, &bottom, first.height)?,
        ),
    };
    refine(first, second, Offset::new(x, y))
}

fn refine(first: &Plane, second: &Plane, offset: Offset) -> Result<Offset, Error> {
    let mut best: Option<(Offset, f32)> = None;
    for y in -REFINE..=REFINE {
        for x in -REFINE..=REFINE {
            let candidate = Offset::new(offset.x + x, offset.y + y);
            let Some(score) = agreement(first, second, candidate) else {
                continue;
            };
            if best.is_none_or(|(_, current)| score > current) {
                best = Some((candidate, score));
            }
        }
    }
    match best {
        Some((offset, score)) if score >= MIN_AGREEMENT => Ok(offset),
        Some((_, score)) => Err(Error::new(format!(
            "the tiles overlap by too little structure to register (correlation {score:.2})"
        ))),
        None => Err(Error::new("the tiles share no overlapping region")),
    }
}

fn agreement(first: &Plane, second: &Plane, offset: Offset) -> Option<f32> {
    let width = first.width as i64;
    let height = first.height as i64;
    let x_start = offset.x.max(0);
    let x_end = (width + offset.x).min(width);
    let y_start = offset.y.max(0);
    let y_end = (height + offset.y).min(height);
    if x_end - x_start < MIN_OVERLAP || y_end - y_start < MIN_OVERLAP {
        return None;
    }
    let mut count = 0.0f64;
    let mut left = 0.0f64;
    let mut right = 0.0f64;
    let mut left_square = 0.0f64;
    let mut right_square = 0.0f64;
    let mut product = 0.0f64;
    for y in y_start..y_end {
        for x in x_start..x_end {
            let a = f64::from(first.values[(x + width * y) as usize]);
            let b = f64::from(second.values[((x - offset.x) + width * (y - offset.y)) as usize]);
            count += 1.0;
            left += a;
            right += b;
            left_square += a * a;
            right_square += b * b;
            product += a * b;
        }
    }
    let covariance = product - left * right / count;
    let variance = (left_square - left * left / count).max(0.0)
        * (right_square - right * right / count).max(0.0);
    let scale = variance.sqrt();
    (scale > 1.0e-9).then(|| (covariance / scale) as f32)
}

pub fn solve(count: usize, edges: &[Edge]) -> Result<Vec<Offset>, Error> {
    assert!(count > 0, "a mosaic holds at least one tile");
    let mut adjacency = vec![Vec::new(); count];
    for (index, edge) in edges.iter().enumerate() {
        assert!(
            edge.first < count && edge.second < count && edge.first != edge.second,
            "a mosaic edge joins two distinct tiles"
        );
        adjacency[edge.first].push((edge.second, index, true));
        adjacency[edge.second].push((edge.first, index, false));
    }
    let mut positions = vec![Offset::default(); count];
    let mut seen = vec![false; count];
    seen[0] = true;
    let mut queue = VecDeque::from([0usize]);
    while let Some(tile) = queue.pop_front() {
        for (neighbour, index, forward) in &adjacency[tile] {
            if seen[*neighbour] {
                continue;
            }
            let offset = edges[*index].offset;
            let shift = if *forward {
                offset
            } else {
                Offset::new(-offset.x, -offset.y)
            };
            positions[*neighbour] =
                Offset::new(positions[tile].x + shift.x, positions[tile].y + shift.y);
            seen[*neighbour] = true;
            queue.push_back(*neighbour);
        }
    }
    if seen.iter().any(|seen| !seen) {
        return Err(Error::new("the tile grid is not connected"));
    }
    let mut x = positions
        .iter()
        .map(|offset| offset.x as f64)
        .collect::<Vec<_>>();
    let mut y = positions
        .iter()
        .map(|offset| offset.y as f64)
        .collect::<Vec<_>>();
    for _ in 0..RELAXATIONS {
        for edge in edges {
            relax(&mut x, edge.first, edge.second, edge.offset.x as f64);
            relax(&mut y, edge.first, edge.second, edge.offset.y as f64);
        }
    }
    Ok(x.iter()
        .zip(&y)
        .map(|(x, y)| Offset::new(x.round() as i64, y.round() as i64))
        .collect())
}

fn relax(values: &mut [f64], first: usize, second: usize, offset: f64) {
    let error = values[second] - values[first] - offset;
    if first == 0 {
        values[second] -= error;
    } else if second == 0 {
        values[first] += error;
    } else {
        values[first] += 0.5 * error;
        values[second] -= 0.5 * error;
    }
}

fn profile(plane: &Plane, x: bool) -> Vec<f32> {
    let length = if x { plane.width } else { plane.height };
    let mut profile = vec![0.0f32; length];
    for (row, values) in plane.values.chunks_exact(plane.width).enumerate() {
        if x {
            for (slot, value) in profile.iter_mut().zip(values) {
                *slot += *value;
            }
        } else {
            profile[row] = values.iter().sum();
        }
    }
    profile
}

fn overlap(a: &[f32], b: &[f32], size: usize) -> Result<i64, Error> {
    let length = size as i64;
    let low = (length / 2).max(1);
    let high = length - MIN_OVERLAP;
    search(a, b, low, high).ok_or_else(|| Error::new("the tiles overlap too little to register"))
}

fn jitter(a: &[f32], b: &[f32], size: usize) -> Result<i64, Error> {
    let length = size as i64;
    let low = -(length / 2);
    let high = length / 2;
    search(a, b, low, high)
        .ok_or_else(|| Error::new("the tiles share too little signal to register"))
}

fn search(a: &[f32], b: &[f32], low: i64, high: i64) -> Option<i64> {
    if high < low {
        return None;
    }
    let mut best: Option<(i64, f32)> = None;
    for lag in low..=high {
        let Some(score) = correlation(a, b, lag) else {
            continue;
        };
        if best.is_none_or(|(_, current)| score > current) {
            best = Some((lag, score));
        }
    }
    match best {
        Some((lag, score)) if score >= MIN_SCORE => Some(lag),
        _ => None,
    }
}

fn correlation(a: &[f32], b: &[f32], lag: i64) -> Option<f32> {
    let length = a.len() as i64;
    let start = lag.max(0);
    let end = (length + lag).min(length);
    if end - start < MIN_OVERLAP {
        return None;
    }
    let mut count = 0.0f64;
    let mut left = 0.0f64;
    let mut right = 0.0f64;
    let mut left_square = 0.0f64;
    let mut right_square = 0.0f64;
    let mut product = 0.0f64;
    for index in start..end {
        let a = f64::from(a[index as usize]);
        let b = f64::from(b[(index - lag) as usize]);
        count += 1.0;
        left += a;
        right += b;
        left_square += a * a;
        right_square += b * b;
        product += a * b;
    }
    let covariance = product - left * right / count;
    let variance = (left_square - left * left / count).max(0.0)
        * (right_square - right * right / count).max(0.0);
    let scale = variance.sqrt();
    (scale > 1.0e-9).then(|| (covariance / scale) as f32)
}
