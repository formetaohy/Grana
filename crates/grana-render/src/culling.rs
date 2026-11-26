use glam::Vec3;
use grana_volume::{BRICK_SIDE, BrickGrid, Level, Region};

use crate::hollow::Mask;

struct Query<'a> {
    level: Level,
    grid: [u32; 3],
    hollow: Option<&'a Mask<'a>>,
}

pub(crate) struct Frustum {
    planes: [[f32; 4]; 6],
    region: Region,
}

impl Frustum {
    pub(crate) fn new(planes: [[f32; 4]; 6], region: Region) -> Self {
        Self { planes, region }
    }

    pub(crate) fn count(&self, level: Level, hollow: Option<&Mask<'_>>) -> usize {
        let grid = BrickGrid::new(level.extent()).counts();
        if grid.contains(&0) {
            return 0;
        }
        let query = Query {
            level,
            grid,
            hollow,
        };
        self.visit(&query, [0; 3], grid)
    }

    pub(crate) fn gather(&self, level: Level) -> Vec<u32> {
        let grid = BrickGrid::new(level.extent()).counts();
        let mut indices = Vec::new();
        if grid.contains(&0) {
            return indices;
        }
        self.collect(level, grid, [0; 3], grid, &mut indices);
        indices
    }

    fn collect(
        &self,
        level: Level,
        grid: [u32; 3],
        low: [u32; 3],
        high: [u32; 3],
        indices: &mut Vec<u32>,
    ) {
        let extent = level.extent();
        let limits = [extent.x(), extent.y(), extent.z()];
        let first = low.map(|value| value * BRICK_SIDE);
        let last = [0, 1, 2].map(|axis| (high[axis] * BRICK_SIDE).min(limits[axis]));
        let (low_world, high_world) = level.cell_bounds(first, last);
        let low_world = Vec3::from(low_world);
        let high_world = Vec3::from(high_world);
        if self.beyond_frustum(low_world, high_world) || self.outside_region(low_world, high_world)
        {
            return;
        }
        let contained =
            self.within_frustum(low_world, high_world) && self.within_region(low_world, high_world);
        if contained {
            for z in low[2]..high[2] {
                for y in low[1]..high[1] {
                    let row = low[0] + grid[0] * (y + grid[1] * z);
                    indices.extend(row..row + (high[0] - low[0]));
                }
            }
            return;
        }
        if brick_count(low, high) == 1 {
            indices.push(low[0] + grid[0] * (low[1] + grid[1] * low[2]));
            return;
        }
        let axis = (0..3)
            .max_by_key(|axis| high[*axis] - low[*axis])
            .expect("a node spans three axes");
        let middle = low[axis] + (high[axis] - low[axis]).div_ceil(2);
        let mut left = high;
        left[axis] = middle;
        let mut right = low;
        right[axis] = middle;
        self.collect(level, grid, low, left, indices);
        self.collect(level, grid, right, high, indices);
    }

    fn visit(&self, query: &Query<'_>, low: [u32; 3], high: [u32; 3]) -> usize {
        let extent = query.level.extent();
        let limits = [extent.x(), extent.y(), extent.z()];
        let first = low.map(|value| value * BRICK_SIDE);
        let last = [0, 1, 2].map(|axis| (high[axis] * BRICK_SIDE).min(limits[axis]));
        let (low_world, high_world) = query.level.cell_bounds(first, last);
        let low_world = Vec3::from(low_world);
        let high_world = Vec3::from(high_world);
        if self.beyond_frustum(low_world, high_world) || self.outside_region(low_world, high_world)
        {
            return 0;
        }
        let bricks = brick_count(low, high);
        let contained =
            self.within_frustum(low_world, high_world) && self.within_region(low_world, high_world);
        if contained {
            return query
                .hollow
                .map_or(bricks, |mask| bricks - mask.count(low, high));
        }
        if bricks == 1 {
            let occupied = query.hollow.is_none_or(|mask| {
                let index = low[0] + query.grid[0] * (low[1] + query.grid[1] * low[2]);
                !mask.holds(index as usize)
            });
            return usize::from(occupied);
        }
        let axis = (0..3)
            .max_by_key(|axis| high[*axis] - low[*axis])
            .expect("a node spans three axes");
        let middle = low[axis] + (high[axis] - low[axis]).div_ceil(2);
        let mut left = high;
        left[axis] = middle;
        let mut right = low;
        right[axis] = middle;
        self.visit(query, low, left) + self.visit(query, right, high)
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
}

fn brick_count(low: [u32; 3], high: [u32; 3]) -> usize {
    (0..3)
        .map(|axis| (high[axis] - low[axis]) as usize)
        .product()
}
