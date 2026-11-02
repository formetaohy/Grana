#[derive(Clone, Debug, PartialEq)]
pub struct Lasso {
    x: usize,
    y: usize,
    points: Vec<[f32; 2]>,
}

impl Lasso {
    pub fn new(x: usize, y: usize, points: Vec<[f32; 2]>) -> Option<Self> {
        let finite = points.iter().flatten().all(|value| value.is_finite());
        (x != y && points.len() >= 3 && finite).then_some(Self { x, y, points })
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
    let cross = (to[0] - from[0]) * (point[1] - from[1]) - (to[1] - from[1]) * (point[0] - from[0]);
    if cross != 0.0 {
        return false;
    }
    let dot = (point[0] - from[0]) * (to[0] - from[0]) + (point[1] - from[1]) * (to[1] - from[1]);
    dot >= 0.0 && dot <= (to[0] - from[0]).powi(2) + (to[1] - from[1]).powi(2)
}
