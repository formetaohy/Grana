use std::collections::VecDeque;

use crate::Error;

const MIN_OVERLAP: i64 = 16;
const MIN_AGREEMENT: f32 = 0.4;
const COARSE_SIDE: usize = 128;
const REFINE_REACH: i64 = 2;
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

    fn reduced(&self, scale: usize) -> Plane {
        if scale <= 1 {
            return self.clone();
        }
        let width = self.width.div_ceil(scale);
        let height = self.height.div_ceil(scale);
        let mut values = vec![0.0f32; width * height];
        for y in 0..self.height {
            for x in 0..self.width {
                values[(x / scale) + width * (y / scale)] += self.values[x + self.width * y];
            }
        }
        let count = (scale * scale) as f32;
        for value in &mut values {
            *value /= count;
        }
        Plane::new(width, height, values)
    }
}

pub fn estimate(first: &Plane, second: &Plane, side: Side) -> Result<Offset, Error> {
    if (first.width, first.height) != (second.width, second.height) {
        return Err(Error::new(
            "the tiles of a mosaic share the extent of their registration plane",
        ));
    }
    let widest = first.width.max(first.height);
    let scale = widest.div_ceil(COARSE_SIDE).max(1);
    let coarse = (first.reduced(scale), second.reduced(scale));
    let overlap = (MIN_OVERLAP / scale as i64).max(2);
    let bounds = search_bounds(coarse.0.width, coarse.0.height, side, overlap);
    let Some((found, _)) = search(&coarse.0, &coarse.1, bounds, overlap) else {
        return Err(Error::new("the tiles share no overlapping region"));
    };
    let guess = Offset::new(found.x * scale as i64, found.y * scale as i64);
    let reach = REFINE_REACH * scale as i64;
    let bounds = search_bounds(first.width, first.height, side, MIN_OVERLAP);
    let window = Bounds {
        x: (guess.x - reach).max(*bounds.x.start())..=(guess.x + reach).min(*bounds.x.end()),
        y: (guess.y - reach).max(*bounds.y.start())..=(guess.y + reach).min(*bounds.y.end()),
    };
    match search(first, second, window, MIN_OVERLAP) {
        Some((offset, score)) if score >= MIN_AGREEMENT => Ok(offset),
        Some((_, score)) => Err(Error::new(format!(
            "the tiles overlap by too little structure to register (correlation {score:.2})"
        ))),
        None => Err(Error::new("the tiles share no overlapping region")),
    }
}

#[derive(Clone, Debug)]
struct Bounds {
    x: std::ops::RangeInclusive<i64>,
    y: std::ops::RangeInclusive<i64>,
}

fn search_bounds(width: usize, height: usize, side: Side, overlap: i64) -> Bounds {
    let (width, height) = (width as i64, height as i64);
    match side {
        Side::Right => Bounds {
            x: (width / 2).max(1)..=width - overlap,
            y: -(height / 2)..=height / 2,
        },
        Side::Below => Bounds {
            x: -(width / 2)..=width / 2,
            y: (height / 2).max(1)..=height - overlap,
        },
    }
}

fn search(first: &Plane, second: &Plane, bounds: Bounds, overlap: i64) -> Option<(Offset, f32)> {
    if bounds.x.is_empty() || bounds.y.is_empty() {
        return None;
    }
    let mut best: Option<(Offset, f32)> = None;
    for y in bounds.y {
        for x in bounds.x.clone() {
            let offset = Offset::new(x, y);
            let Some(score) = agreement(first, second, offset, overlap) else {
                continue;
            };
            if best.is_none_or(|(_, current)| score > current) {
                best = Some((offset, score));
            }
        }
    }
    best
}

fn agreement(first: &Plane, second: &Plane, offset: Offset, overlap: i64) -> Option<f32> {
    let width = first.width as i64;
    let height = first.height as i64;
    let x_start = offset.x.max(0);
    let x_end = (width + offset.x).min(width);
    let y_start = offset.y.max(0);
    let y_end = (height + offset.y).min(height);
    if x_end - x_start < overlap || y_end - y_start < overlap {
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
