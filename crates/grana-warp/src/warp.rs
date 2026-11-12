use std::sync::Arc;

use crate::grid::{DIGEST_BASIS, Grid, mix};

#[derive(Clone, Debug)]
pub enum Warp {
    Shift([f32; 3]),
    Field(Arc<Grid>),
}

impl Warp {
    pub const IDENTITY: Self = Self::Shift([0.0; 3]);

    pub fn shift(offset: [f32; 3]) -> Self {
        Self::Shift(offset)
    }

    pub fn field(grid: Grid) -> Self {
        Self::Field(Arc::new(grid))
    }

    pub fn sample(&self, point: [f32; 3]) -> [f32; 3] {
        match self {
            Self::Shift(offset) => *offset,
            Self::Field(grid) => grid.sample(point),
        }
    }

    pub fn translation(&self) -> [f32; 3] {
        match self {
            Self::Shift(offset) => *offset,
            Self::Field(grid) => grid.translation(),
        }
    }

    pub fn magnitude(&self) -> f32 {
        match self {
            Self::Shift(offset) => offset.iter().map(|value| value.abs()).fold(0.0, f32::max),
            Self::Field(grid) => grid.magnitude(),
        }
    }

    pub fn field_grid(&self) -> Option<&Grid> {
        match self {
            Self::Shift(_) => None,
            Self::Field(grid) => Some(grid),
        }
    }

    pub fn deformed(&self) -> bool {
        matches!(self, Self::Field(_))
    }

    pub fn identity(&self) -> bool {
        match self {
            Self::Shift(offset) => *offset == [0.0; 3],
            Self::Field(_) => false,
        }
    }

    pub fn added(&self, other: &Self) -> Self {
        match (self.identity(), other.identity()) {
            (true, _) => other.clone(),
            (_, true) => self.clone(),
            (false, false) => match (self, other) {
                (Self::Shift(left), Self::Shift(right)) => {
                    Self::Shift([0, 1, 2].map(|axis| left[axis] + right[axis]))
                }
                (Self::Field(grid), Self::Shift(offset))
                | (Self::Shift(offset), Self::Field(grid)) => {
                    Self::Field(Arc::new(grid.offset(*offset)))
                }
                (Self::Field(grid), Self::Field(other)) => match grid.same_lattice(other) {
                    true => Self::Field(Arc::new(grid.combine(other, |left, right| {
                        [0, 1, 2].map(|axis| left[axis] + right[axis])
                    }))),
                    false => Self::Field(Arc::new(Grid::sampled(
                        grid.dims(),
                        grid.origin(),
                        grid.step(),
                        |point| {
                            let left = self.sample(point);
                            let right = other.sample(point);
                            [0, 1, 2].map(|axis| left[axis] + right[axis])
                        },
                    ))),
                },
            },
        }
    }

    pub fn translated(&self, delta: [f32; 3]) -> Self {
        match self {
            Self::Shift(offset) => Self::Shift([0, 1, 2].map(|axis| offset[axis] + delta[axis])),
            Self::Field(grid) => Self::Field(Arc::new(grid.offset(delta))),
        }
    }

    pub fn relative_to(&self, reference: &Self) -> Self {
        match (self, reference) {
            (Self::Shift(offset), Self::Shift(anchor)) => {
                Self::Shift([0, 1, 2].map(|axis| offset[axis] - anchor[axis]))
            }
            (Self::Field(grid), Self::Shift(anchor)) => {
                Self::Field(Arc::new(grid.offset([0, 1, 2].map(|axis| -anchor[axis]))))
            }
            (Self::Shift(offset), Self::Field(anchor)) => Self::Field(Arc::new(
                anchor.map(|value| [0, 1, 2].map(|axis| offset[axis] - value[axis])),
            )),
            (Self::Field(grid), Self::Field(anchor)) => {
                Self::Field(Arc::new(grid.combine(anchor, |value, anchored| {
                    [0, 1, 2].map(|axis| value[axis] - anchored[axis])
                })))
            }
        }
    }

    pub fn digest(&self) -> u64 {
        match self {
            Self::Shift(offset) => {
                let mut digest = mix(DIGEST_BASIS, 0);
                for value in offset {
                    digest = mix(digest, u64::from(value.to_bits()));
                }
                digest
            }
            Self::Field(grid) => mix(DIGEST_BASIS, grid.digest()),
        }
    }
}

impl PartialEq for Warp {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Shift(left), Self::Shift(right)) => left == right,
            (Self::Field(left), Self::Field(right)) => left == right,
            _ => false,
        }
    }
}
