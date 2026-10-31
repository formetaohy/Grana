#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Filter {
    column: usize,
    minimum: f32,
    maximum: f32,
}

impl Filter {
    pub fn new(column: usize, minimum: f32, maximum: f32) -> Self {
        assert!(
            minimum.is_finite() && maximum.is_finite(),
            "a filter range is finite"
        );
        assert!(minimum <= maximum, "a filter range is ordered");
        Self {
            column,
            minimum,
            maximum,
        }
    }

    pub fn column(self) -> usize {
        self.column
    }

    pub fn minimum(self) -> f32 {
        self.minimum
    }

    pub fn maximum(self) -> f32 {
        self.maximum
    }

    pub fn contains(self, value: f32) -> bool {
        self.minimum <= value && value <= self.maximum
    }
}
