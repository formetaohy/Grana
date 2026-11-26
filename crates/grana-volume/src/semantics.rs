#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VoxelSemantics {
    Scalar,
    Label,
    Presence,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Reduction {
    Average,
    Dominant,
    Presence,
}

impl Reduction {
    pub fn averages(self) -> bool {
        matches!(self, Self::Average)
    }

    pub fn code(self) -> u8 {
        match self {
            Self::Average => 0,
            Self::Dominant => 1,
            Self::Presence => 2,
        }
    }

    pub fn combine(self, values: &[f64], fill: f64) -> f64 {
        assert!(
            !values.is_empty(),
            "a reduced voxel combines the voxels it covers"
        );
        match self {
            Self::Average => values.iter().sum::<f64>() / values.len() as f64,
            Self::Presence => values
                .iter()
                .copied()
                .filter(|value| *value != fill)
                .fold(fill, f64::max),
            Self::Dominant => dominant(values, fill),
        }
    }
}

fn dominant(values: &[f64], fill: f64) -> f64 {
    let mut counts = [(0u32, 0.0f64); 8];
    let mut distinct = 0usize;
    for value in values.iter().copied().filter(|value| *value != fill) {
        match counts[..distinct]
            .iter_mut()
            .find(|(_, held)| *held == value)
        {
            Some((count, _)) => *count += 1,
            None => {
                counts[distinct] = (1, value);
                distinct += 1;
            }
        }
    }
    counts[..distinct]
        .iter()
        .max_by(|left, right| left.0.cmp(&right.0).then(right.1.total_cmp(&left.1)))
        .map_or(fill, |(_, value)| *value)
}

impl VoxelSemantics {
    pub fn code(self) -> u8 {
        match self {
            Self::Scalar => 0,
            Self::Label => 1,
            Self::Presence => 2,
        }
    }

    pub fn reduction(self) -> Reduction {
        match self {
            Self::Scalar => Reduction::Average,
            Self::Label => Reduction::Dominant,
            Self::Presence => Reduction::Presence,
        }
    }
}
