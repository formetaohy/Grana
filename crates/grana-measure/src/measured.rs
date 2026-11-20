#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Measured {
    Value { value: f32, support: u32 },
    Absent,
}

impl Measured {
    pub fn new(value: f32, support: u32) -> Self {
        assert!(value.is_finite(), "a measured value is a finite number");
        assert!(support > 0, "a measured value rests on one sample");
        Self::Value { value, support }
    }

    pub const ABSENT: Self = Self::Absent;

    pub fn absent() -> Self {
        Self::Absent
    }

    pub fn present(self) -> Option<f32> {
        match self {
            Self::Value { value, .. } => Some(value),
            Self::Absent => None,
        }
    }

    pub fn support(self) -> u32 {
        match self {
            Self::Value { support, .. } => support,
            Self::Absent => 0,
        }
    }

    pub fn is_present(self) -> bool {
        matches!(self, Self::Value { .. })
    }

    pub fn is_present_and(self, test: impl FnOnce(f32) -> bool) -> bool {
        match self {
            Self::Value { value, .. } => test(value),
            Self::Absent => false,
        }
    }

    pub fn total_cmp(self, other: Self) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        match (self, other) {
            (Self::Value { value: left, .. }, Self::Value { value: right, .. }) => {
                left.total_cmp(&right)
            }
            (Self::Absent, Self::Absent) => Ordering::Equal,
            (Self::Absent, Self::Value { .. }) => Ordering::Greater,
            (Self::Value { .. }, Self::Absent) => Ordering::Less,
        }
    }

    pub fn map(self, apply: impl FnOnce(f32) -> f32) -> Self {
        match self {
            Self::Value { value, support } => Self::new(apply(value), support),
            Self::Absent => Self::Absent,
        }
    }

    pub fn text(self, decimals: u8) -> Option<String> {
        self.present()
            .map(|value| format!("{:.*}", decimals as usize, value))
    }
}

impl From<Option<(f32, u32)>> for Measured {
    fn from(held: Option<(f32, u32)>) -> Self {
        match held {
            Some((value, support)) => Self::new(value, support),
            None => Self::Absent,
        }
    }
}
