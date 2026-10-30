#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    min: [f32; 3],
    max: [f32; 3],
}

impl Bounds {
    pub fn new(min: [f32; 3], max: [f32; 3]) -> Self {
        assert!(
            min.iter().chain(max.iter()).all(|value| value.is_finite()),
            "bounds are finite"
        );
        assert!(
            (0..3).all(|axis| min[axis] <= max[axis]),
            "bounds are ordered"
        );
        Self { min, max }
    }

    pub fn min(self) -> [f32; 3] {
        self.min
    }

    pub fn max(self) -> [f32; 3] {
        self.max
    }

    pub fn center(self) -> [f32; 3] {
        [0, 1, 2].map(|axis| 0.5 * (self.min[axis] + self.max[axis]))
    }

    pub fn size(self) -> [f32; 3] {
        [0, 1, 2].map(|axis| self.max[axis] - self.min[axis])
    }

    pub fn contains(self, point: [f32; 3]) -> bool {
        (0..3).all(|axis| self.min[axis] <= point[axis] && point[axis] <= self.max[axis])
    }
}
