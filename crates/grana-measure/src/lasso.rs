use crate::measured::Measured;

#[derive(Clone, Debug, PartialEq)]
pub struct Lasso {
    x: usize,
    y: usize,
    points: Vec<[f32; 2]>,
}

impl Lasso {
    pub fn new(x: usize, y: usize, points: Vec<[f32; 2]>) -> Option<Self> {
        let finite = points.iter().flatten().all(|value| value.is_finite());
        if x == y || !finite {
            return None;
        }
        let mut ring = Vec::with_capacity(points.len());
        for point in points {
            if ring.last() != Some(&point) {
                ring.push(point);
            }
        }
        if ring.len() > 1 && ring.first() == ring.last() {
            ring.pop();
        }
        (ring.len() >= 3).then_some(Self { x, y, points: ring })
    }

    pub fn x(&self) -> usize {
        self.x
    }

    pub fn y(&self) -> usize {
        self.y
    }

    pub fn points(&self) -> &[[f32; 2]] {
        &self.points
    }

    pub fn admits(&self, x: Measured, y: Measured) -> bool {
        match (x.present(), y.present()) {
            (Some(x), Some(y)) => self.contains(x, y),
            _ => false,
        }
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        if !(x.is_finite() && y.is_finite()) {
            return false;
        }
        let mut inside = false;
        let mut previous = self.points[self.points.len() - 1];
        for point in &self.points {
            if on_segment(previous, *point, [x, y]) {
                return true;
            }
            if (point[1] > y) != (previous[1] > y) {
                let crossing =
                    point[0] + (y - point[1]) / (previous[1] - point[1]) * (previous[0] - point[0]);
                if x < crossing {
                    inside = !inside;
                }
            }
            previous = *point;
        }
        inside
    }
}

fn on_segment(from: [f32; 2], to: [f32; 2], point: [f32; 2]) -> bool {
    let span = [to[0] - from[0], to[1] - from[1]];
    let length = span[0] * span[0] + span[1] * span[1];
    if length == 0.0 {
        return point == from;
    }
    let cross = span[0] * (point[1] - from[1]) - span[1] * (point[0] - from[0]);
    if cross != 0.0 {
        return false;
    }
    let dot = (point[0] - from[0]) * span[0] + (point[1] - from[1]) * span[1];
    dot >= 0.0 && dot <= length
}
