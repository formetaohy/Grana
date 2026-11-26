pub(crate) struct Grid<'a> {
    points: &'a [[f32; 3]],
    low: [f32; 3],
    cell: f32,
    dims: [usize; 3],
    slots: Vec<Vec<u32>>,
}

impl<'a> Grid<'a> {
    pub(crate) fn new(points: &'a [[f32; 3]], cell: f32) -> Self {
        assert!(
            cell.is_finite() && cell > 0.0,
            "a spatial grid holds cells of a positive width"
        );
        let mut low = [f32::INFINITY; 3];
        let mut high = [f32::NEG_INFINITY; 3];
        for point in points {
            for axis in 0..3 {
                low[axis] = low[axis].min(point[axis]);
                high[axis] = high[axis].max(point[axis]);
            }
        }
        let low = low.map(|value| if value.is_finite() { value } else { 0.0 });
        let high = high.map(|value| if value.is_finite() { value } else { 0.0 });
        let dims = [0, 1, 2].map(|axis| {
            (((high[axis] - low[axis]) / cell).floor() as usize)
                .saturating_add(1)
                .max(1)
        });
        let mut grid = Self {
            points,
            low,
            cell,
            dims,
            slots: vec![Vec::new(); dims[0] * dims[1] * dims[2]],
        };
        for (index, point) in points.iter().enumerate() {
            let slot = grid.slot(grid.coord(*point));
            grid.slots[slot].push(index as u32);
        }
        grid
    }

    fn coord(&self, point: [f32; 3]) -> [i32; 3] {
        [0, 1, 2].map(|axis| {
            let unit = ((point[axis] - self.low[axis]) / self.cell).floor();
            (unit as i32).clamp(0, self.dims[axis] as i32 - 1)
        })
    }

    fn slot(&self, coord: [i32; 3]) -> usize {
        (coord[2] as usize * self.dims[1] + coord[1] as usize) * self.dims[0] + coord[0] as usize
    }

    pub(crate) fn within(&self, point: [f32; 3], radius: f32, mut visit: impl FnMut(usize, f32)) {
        if !(radius.is_finite() && radius > 0.0) {
            return;
        }
        let reach = (radius / self.cell).ceil() as i32;
        let origin = self.coord(point);
        let squared = radius * radius;
        let bounds = [0, 1, 2].map(|axis| {
            [
                (origin[axis] - reach).max(0),
                (origin[axis] + reach).min(self.dims[axis] as i32 - 1),
            ]
        });
        for z in bounds[2][0]..=bounds[2][1] {
            for y in bounds[1][0]..=bounds[1][1] {
                for x in bounds[0][0]..=bounds[0][1] {
                    for index in &self.slots[self.slot([x, y, z])] {
                        let other = self.points[*index as usize];
                        let distance = (0..3)
                            .map(|axis| {
                                let delta = other[axis] - point[axis];
                                delta * delta
                            })
                            .sum::<f32>();
                        if distance <= squared {
                            visit(*index as usize, distance.sqrt());
                        }
                    }
                }
            }
        }
    }

    pub(crate) fn nearest(&self, point: [f32; 3], limit: f32, skip: Option<usize>) -> Option<f32> {
        let mut reach = self.cell;
        loop {
            let bound = reach.min(limit);
            let mut best = f32::INFINITY;
            self.within(point, bound, |index, distance| {
                if Some(index) != skip && distance < best {
                    best = distance;
                }
            });
            if best.is_finite() {
                return Some(best);
            }
            if bound >= limit || bound >= self.farthest(point) {
                return None;
            }
            reach *= 2.0;
        }
    }

    fn farthest(&self, point: [f32; 3]) -> f32 {
        let mut farthest = 0.0f32;
        for corner in 0..8 {
            let mut squared = 0.0f32;
            for axis in 0..3 {
                let edge = if corner & (1 << axis) == 0 {
                    self.low[axis]
                } else {
                    self.low[axis] + self.dims[axis] as f32 * self.cell
                };
                let delta = edge - point[axis];
                squared += delta * delta;
            }
            farthest = farthest.max(squared.sqrt());
        }
        farthest
    }
}
