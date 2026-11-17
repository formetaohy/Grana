use glam::Vec3;
use grana_volume::{BRICK_SIDE, BrickGrid, Level, Region};

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

    pub(crate) fn count(&self, level: Level, lower: f32, upper: f32) -> usize {
        let counts = BrickGrid::new(level.extent()).counts();
        if counts.contains(&0) {
            return 0;
        }
        self.count_node(level, [0; 3], counts, lower, upper)
    }

    fn count_node(
        &self,
        level: Level,
        low: [u32; 3],
        high: [u32; 3],
        lower: f32,
        upper: f32,
    ) -> usize {
        let extent = level.extent();
        let limits = [extent.x(), extent.y(), extent.z()];
        let first = low.map(|value| value * BRICK_SIDE);
        let last = [0, 1, 2].map(|axis| (high[axis] * BRICK_SIDE).min(limits[axis]));
        let (low_world, high_world) = level.cell_bounds(first, last);
        let low_world = Vec3::from(low_world);
        let high_world = Vec3::from(high_world);
        if self.beyond_frustum(low_world, high_world) || self.outside_region(low_world, high_world)
        {
            return 0;
        }
        let (near, far) = self.reach(low_world, high_world);
        if far < lower || near >= upper {
            return 0;
        }
        let bricks = (0..3)
            .map(|axis| (high[axis] - low[axis]) as usize)
            .product();
        if bricks == 1 {
            return 1;
        }
        if near >= lower
            && far < upper
            && self.within_frustum(low_world, high_world)
            && self.within_region(low_world, high_world)
        {
            return bricks;
        }
        let axis = (0..3)
            .max_by_key(|axis| high[*axis] - low[*axis])
            .expect("a node spans three axes");
        let middle = low[axis] + (high[axis] - low[axis]).div_ceil(2);
        let mut left_high = high;
        left_high[axis] = middle;
        let mut right_low = low;
        right_low[axis] = middle;
        self.count_node(level, low, left_high, lower, upper)
            + self.count_node(level, right_low, high, lower, upper)
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
