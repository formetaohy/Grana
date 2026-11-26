use grana_volume::PADDED_SIDE;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Slots {
    grid: [u32; 3],
    capacity: usize,
}

impl Slots {
    pub(crate) fn limit(max_dimension: u32) -> usize {
        limit(max_dimension)
    }

    pub(crate) fn fitting(wanted: usize, max_dimension: u32) -> Self {
        let limit = limit(max_dimension);
        let wanted = wanted.clamp(1, limit * limit * limit);
        let mut best = Self {
            grid: [1, 1, 1],
            capacity: 1,
        };
        for depth in 1..=limit {
            let Some(per) = wanted.checked_div(depth).filter(|per| *per > 0) else {
                break;
            };
            let side = isqrt(per).min(limit);
            let capacity = side * side * depth;
            if capacity <= wanted && capacity > best.capacity {
                best = Self::new(side, depth);
            }
        }
        best
    }

    pub(crate) fn covering(wanted: usize, max_dimension: u32) -> Result<Self, String> {
        let limit = limit(max_dimension);
        let held = limit * limit * limit;
        if wanted > held {
            return Err(format!(
                "the render needs {wanted} brick slots at full detail, but this device holds at most {held}"
            ));
        }
        let wanted = wanted.max(1);
        let mut best: Option<Self> = None;
        for depth in 1..=limit {
            let per = wanted.div_ceil(depth);
            let side = ceil_sqrt(per).min(limit);
            let capacity = side * side * depth;
            if capacity >= wanted && best.is_none_or(|held| capacity <= held.capacity) {
                best = Some(Self::new(side, depth));
            }
            if depth >= wanted {
                break;
            }
        }
        best.ok_or_else(|| "the device cannot hold a brick slot".to_string())
    }

    fn new(side: usize, depth: usize) -> Self {
        Self {
            grid: [side as u32, side as u32, depth as u32],
            capacity: side * side * depth,
        }
    }

    pub(crate) fn grid(self) -> [u32; 3] {
        self.grid
    }

    pub(crate) fn capacity(self) -> usize {
        self.capacity
    }
}

fn limit(max_dimension: u32) -> usize {
    (max_dimension / PADDED_SIDE).max(1) as usize
}

fn isqrt(value: usize) -> usize {
    let mut root = (value as f64).sqrt() as usize;
    while root > 0 && root * root > value {
        root -= 1;
    }
    while (root + 1) * (root + 1) <= value {
        root += 1;
    }
    root.max(1)
}

fn ceil_sqrt(value: usize) -> usize {
    let root = isqrt(value);
    if root * root == value { root } else { root + 1 }
}
