use glam::Vec3;
use grana_volume::{BRICK_SIDE, BrickGrid, Level, Region};

const WHOLE: usize = 0;
const FIRST: usize = 1;
const MIDDLE: usize = 2;
const KINDS: usize = 3;
const ALL: u8 = (1 << KINDS) - 1;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Counts([usize; KINDS]);

impl Counts {
    pub(crate) fn band(&self, band: usize, count: usize) -> usize {
        if count == 1 || band + 1 == count {
            self.0[WHOLE]
        } else if band == 0 {
            self.0[FIRST]
        } else {
            self.0[MIDDLE]
        }
    }
}

struct Bands {
    lower: [f32; KINDS],
    upper: [f32; KINDS],
}

impl Bands {
    fn new(lower: f32, upper: f32) -> Self {
        Self {
            lower: [f32::NEG_INFINITY, f32::NEG_INFINITY, lower],
            upper: [f32::INFINITY, upper, upper],
        }
    }
}

pub(crate) struct Frustum {
    planes: [[f32; 4]; 6],
    eye: Vec3,
    region: Region,
}

impl Frustum {
    pub(crate) fn new(planes: [[f32; 4]; 6], eye: Vec3, region: Region) -> Self {
        Self {
            planes,
            eye,
            region,
        }
    }

    pub(crate) fn counts(&self, level: Level, lower: f32, upper: f32) -> Counts {
        let counts = BrickGrid::new(level.extent()).counts();
        if counts.contains(&0) {
            return Counts::default();
        }
        let bands = Bands::new(lower, upper);
        Counts(self.visit(level, [0; 3], counts, &bands, ALL).0)
    }

    fn visit(
        &self,
        level: Level,
        low: [u32; 3],
        high: [u32; 3],
        bands: &Bands,
        open: u8,
    ) -> Counts {
        let extent = level.extent();
        let limits = [extent.x(), extent.y(), extent.z()];
        let first = low.map(|value| value * BRICK_SIDE);
        let last = [0, 1, 2].map(|axis| (high[axis] * BRICK_SIDE).min(limits[axis]));
        let (low_world, high_world) = level.cell_bounds(first, last);
        let low_world = Vec3::from(low_world);
        let high_world = Vec3::from(high_world);
        if self.beyond_frustum(low_world, high_world) || self.outside_region(low_world, high_world)
        {
            return Counts::default();
        }
        let (near, far) = self.reach(low_world, high_world);
        let bricks = (0..3)
            .map(|axis| (high[axis] - low[axis]) as usize)
            .product::<usize>();
        let contained =
            self.within_frustum(low_world, high_world) && self.within_region(low_world, high_world);
        let mut counts = [0usize; KINDS];
        let mut descend = 0u8;
        for (kind, slot) in counts.iter_mut().enumerate() {
            if open & (1 << kind) == 0 {
                continue;
            }
            let lower = bands.lower[kind];
            let upper = bands.upper[kind];
            if far < lower || near >= upper {
                continue;
            }
            if contained && near >= lower && far < upper {
                *slot += bricks;
                continue;
            }
            descend |= 1 << kind;
        }
        if descend == 0 {
            return Counts(counts);
        }
        if bricks == 1 {
            for (kind, slot) in counts.iter_mut().enumerate() {
                if descend & (1 << kind) != 0 {
                    *slot += 1;
                }
            }
            return Counts(counts);
        }
        let axis = (0..3)
            .max_by_key(|axis| high[*axis] - low[*axis])
            .expect("a node spans three axes");
        let middle = low[axis] + (high[axis] - low[axis]).div_ceil(2);
        let mut left = high;
        left[axis] = middle;
        let mut right = low;
        right[axis] = middle;
        for (low, high) in [(low, left), (right, high)] {
            let part = self.visit(level, low, high, bands, descend);
            for (count, other) in counts.iter_mut().zip(part.0) {
                *count += other;
            }
        }
        Counts(counts)
    }

    fn beyond_frustum(&self, low: Vec3, high: Vec3) -> bool {
        self.planes.iter().any(|plane| {
            let direction = Vec3::from_array([plane[0], plane[1], plane[2]]);
            let positive = Vec3::new(
                if plane[0] >= 0.0 { high.x } else { low.x },
                if plane[1] >= 0.0 { high.y } else { low.y },
                if plane[2] >= 0.0 { high.z } else { low.z },
            );
            direction.dot(positive) + plane[3] < 0.0
        })
    }

    fn within_frustum(&self, low: Vec3, high: Vec3) -> bool {
        self.planes.iter().all(|plane| {
            let direction = Vec3::from_array([plane[0], plane[1], plane[2]]);
            let negative = Vec3::new(
                if plane[0] >= 0.0 { low.x } else { high.x },
                if plane[1] >= 0.0 { low.y } else { high.y },
                if plane[2] >= 0.0 { low.z } else { high.z },
            );
            direction.dot(negative) + plane[3] >= 0.0
        })
    }

    fn outside_region(&self, low: Vec3, high: Vec3) -> bool {
        if !self.region.enabled() {
            return false;
        }
        let half = self.region.half();
        let middle = 0.5 * (low + high);
        let extent = 0.5 * (high - low);
        let offset = middle - self.region.center();
        (0..3).any(|axis| {
            let direction = self.region.axis(axis);
            let reach = extent.dot(direction.abs());
            offset.dot(direction).abs() > half[axis] + reach
        })
    }

    fn within_region(&self, low: Vec3, high: Vec3) -> bool {
        if !self.region.enabled() {
            return true;
        }
        let half = self.region.half();
        let middle = 0.5 * (low + high);
        let extent = 0.5 * (high - low);
        let offset = middle - self.region.center();
        (0..3).all(|axis| {
            let direction = self.region.axis(axis);
            let reach = extent.dot(direction.abs());
            offset.dot(direction).abs() + reach <= half[axis]
        })
    }

    fn reach(&self, low: Vec3, high: Vec3) -> (f32, f32) {
        let eye = self.eye;
        let near = (eye.clamp(low, high) - eye).length();
        let far = (eye - low).abs().max((eye - high).abs()).length();
        (near, far)
    }
}
