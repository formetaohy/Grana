use grana_geometry::PackedMesh;
use grana_volume::Level;

use crate::shape::Shape;

const NUDGE: [f32; 2] = [1.0e-4, 3.7e-4];
const WORD: usize = 64;
const PARALLEL: f32 = 1.0e-12;

type Triangle = [[f32; 3]; 3];

pub struct Mask {
    origin: [i64; 3],
    extent: [u32; 3],
    bits: Vec<u64>,
    spacing: [f32; 3],
    world: [f32; 3],
}

impl Mask {
    pub fn empty(level: Level) -> Self {
        let spacing = level.spacing();
        let world = level.world_extent();
        Self {
            origin: [0; 3],
            extent: [0; 3],
            bits: Vec::new(),
            spacing: [spacing.x(), spacing.y(), spacing.z()],
            world,
        }
    }

    pub fn contains(&self, point: [f32; 3]) -> bool {
        let cell = [0, 1, 2].map(|axis| {
            ((point[axis] + 0.5 * self.world[axis]) / self.spacing[axis]).floor() as i64
        });
        self.contains_cell_index(&cell)
    }

    pub fn contains_cell(&self, cell: [u32; 3]) -> bool {
        self.contains_cell_index(&cell.map(i64::from))
    }

    fn contains_cell_index(&self, cell: &[i64; 3]) -> bool {
        if !self.inside(cell) {
            return false;
        }
        let index = self.index(cell);
        self.bits[index / WORD] & (1 << (index % WORD)) != 0
    }

    pub fn count(&self) -> u64 {
        self.bits
            .iter()
            .map(|word| u64::from(word.count_ones()))
            .sum()
    }

    pub fn is_empty(&self) -> bool {
        self.extent.contains(&0)
    }

    fn inside(&self, cell: &[i64; 3]) -> bool {
        (0..3).all(|axis| {
            let low = self.origin[axis];
            cell[axis] >= low && cell[axis] < low + i64::from(self.extent[axis])
        })
    }

    fn index(&self, cell: &[i64; 3]) -> usize {
        let local = [0, 1, 2].map(|axis| (cell[axis] - self.origin[axis]) as usize);
        local[0] + self.extent[0] as usize * (local[1] + self.extent[1] as usize * local[2])
    }
}

pub struct Roi {
    frames: Vec<Option<Shape>>,
}

impl Roi {
    pub fn new(frames: Vec<Option<Shape>>) -> Self {
        Self { frames }
    }

    pub fn mask(&self, level: Level, time: u32) -> Mask {
        let Some(Some(frame)) = self.frames.get(time as usize) else {
            return Mask::empty(level);
        };
        let Some((low, high)) = frame.bounds() else {
            return Mask::empty(level);
        };
        let spacing = level.spacing();
        let spacing = [spacing.x(), spacing.y(), spacing.z()];
        let world = level.world_extent();
        let extent = level.extent();
        let extent = [extent.x(), extent.y(), extent.z()];
        let continuous =
            |point: [f32; 3], axis: usize| (point[axis] + 0.5 * world[axis]) / spacing[axis];
        let mut origin = [0i64; 3];
        let mut limit = [0i64; 3];
        for (axis, slot) in extent.iter().enumerate() {
            origin[axis] = (continuous(low, axis).floor() as i64 - 1).max(0);
            limit[axis] = (continuous(high, axis).ceil() as i64 + 1).min(i64::from(*slot));
        }
        if (0..3).any(|axis| limit[axis] <= origin[axis]) {
            return Mask::empty(level);
        }
        let mut area = Area::new(origin, limit, spacing, world);
        match frame {
            Shape::Spots(spots) => {
                for spot in spots.iter() {
                    area.sphere(spot.center(), spot.radius());
                }
            }
            Shape::Mesh(mesh) => rasterize(mesh, &mut area),
        }
        area.finish()
    }
}

struct Area {
    origin: [i64; 3],
    extent: [u32; 3],
    bits: Vec<u64>,
    spacing: [f32; 3],
    world: [f32; 3],
}

impl Area {
    fn new(origin: [i64; 3], limit: [i64; 3], spacing: [f32; 3], world: [f32; 3]) -> Self {
        let extent = [0, 1, 2].map(|axis| (limit[axis] - origin[axis]) as u32);
        let cells = extent
            .iter()
            .map(|value| u64::from(*value))
            .product::<u64>();
        let words = cells.div_ceil(WORD as u64) as usize;
        Self {
            origin,
            extent,
            bits: vec![0; words],
            spacing,
            world,
        }
    }

    fn continuous(&self, point: [f32; 3]) -> [f32; 3] {
        [0, 1, 2].map(|axis| (point[axis] + 0.5 * self.world[axis]) / self.spacing[axis])
    }

    fn sphere(&mut self, center: [f32; 3], radius: f32) {
        if !(radius.is_finite() && radius > 0.0) {
            return;
        }
        let center = self.continuous(center);
        let mut low = [0i64; 3];
        let mut high = [0i64; 3];
        for axis in 0..3 {
            let reach = radius / self.spacing[axis];
            low[axis] = self.clip_axis((center[axis] - reach).floor() as i64, axis);
            high[axis] = self.clip_axis((center[axis] + reach).ceil() as i64 + 1, axis);
            if high[axis] <= low[axis] {
                return;
            }
        }
        let mut cell = low;
        while cell[2] < high[2] {
            cell[1] = low[1];
            while cell[1] < high[1] {
                cell[0] = low[0];
                while cell[0] < high[0] {
                    let offset = [0, 1, 2].map(|axis| {
                        ((cell[axis] as f32 + 0.5) - center[axis]) * self.spacing[axis]
                    });
                    if offset.iter().map(|value| value * value).sum::<f32>() <= radius * radius {
                        self.set(cell);
                    }
                    cell[0] += 1;
                }
                cell[1] += 1;
            }
            cell[2] += 1;
        }
    }

    fn set(&mut self, cell: [i64; 3]) {
        let local = [0, 1, 2].map(|axis| cell[axis] - self.origin[axis]);
        if (0..3).any(|axis| local[axis] < 0 || local[axis] >= i64::from(self.extent[axis])) {
            return;
        }
        let local = local.map(|value| value as usize);
        let index =
            local[0] + self.extent[0] as usize * (local[1] + self.extent[1] as usize * local[2]);
        self.bits[index / WORD] |= 1 << (index % WORD);
    }

    fn clip_axis(&self, cell: i64, axis: usize) -> i64 {
        cell.clamp(
            self.origin[axis],
            self.origin[axis] + i64::from(self.extent[axis]),
        )
    }

    fn finish(self) -> Mask {
        Mask {
            origin: self.origin,
            extent: self.extent,
            bits: self.bits,
            spacing: self.spacing,
            world: self.world,
        }
    }
}

fn rasterize(mesh: &PackedMesh, area: &mut Area) {
    if mesh.is_empty() {
        return;
    }
    for object in 0..mesh.object_count() {
        let triangles = mesh.groups()[object].index_count() as usize / 3;
        if triangles == 0 {
            continue;
        }
        let points = (0..triangles)
            .map(|index| {
                mesh.triangle(object, index)
                    .map(|point| area.continuous(point))
            })
            .collect::<Vec<_>>();
        cut(&points, area);
    }
}

fn cut(triangles: &[Triangle], area: &mut Area) {
    let (plane, across, down) = dominant(triangles);
    let mut low = [i64::MAX; 2];
    let mut high = [i64::MIN; 2];
    for triangle in triangles {
        for (slot, axis) in [across, down].into_iter().enumerate() {
            for point in triangle {
                let cell = point[axis].floor() as i64;
                low[slot] = low[slot].min(cell);
                high[slot] = high[slot].max(cell);
            }
        }
    }
    let mut origin = [0i64; 3];
    let mut limit = [0i64; 3];
    for (slot, axis) in [across, down].into_iter().enumerate() {
        origin[slot] = area.clip_axis(low[slot], axis);
        limit[slot] = area.clip_axis(high[slot] + 1, axis);
    }
    let columns = [0, 1].map(|slot| (limit[slot] - origin[slot]).max(0) as usize);
    if columns.contains(&0) {
        return;
    }
    let mut buckets = vec![Vec::new(); columns[0] * columns[1]];
    for (index, triangle) in triangles.iter().enumerate() {
        let mut range = [(0usize, 0usize); 2];
        let mut skipped = false;
        for (slot, axis) in [across, down].into_iter().enumerate() {
            let minimum = triangle
                .iter()
                .map(|point| point[axis])
                .fold(f32::INFINITY, f32::min);
            let maximum = triangle
                .iter()
                .map(|point| point[axis])
                .fold(f32::NEG_INFINITY, f32::max);
            let first = (minimum.floor() as i64).max(origin[slot]);
            let last = (maximum.floor() as i64).min(limit[slot] - 1);
            if last < first {
                skipped = true;
                break;
            }
            range[slot] = (
                (first - origin[slot]) as usize,
                (last - origin[slot]) as usize,
            );
        }
        if skipped {
            continue;
        }
        for down_index in range[1].0..=range[1].1 {
            for across_index in range[0].0..=range[0].1 {
                buckets[across_index + columns[0] * down_index].push(index as u32);
            }
        }
    }
    let mut crossings: Vec<f32> = Vec::new();
    for down_index in 0..columns[1] {
        for across_index in 0..columns[0] {
            let bucket = &buckets[across_index + columns[0] * down_index];
            if bucket.is_empty() {
                continue;
            }
            let sample = [
                origin[0] as f32 + across_index as f32 + 0.5 + NUDGE[0],
                origin[1] as f32 + down_index as f32 + 0.5 + NUDGE[1],
            ];
            crossings.clear();
            for index in bucket {
                let triangle = &triangles[*index as usize];
                let first = triangle[0];
                let second = triangle[1];
                let third = triangle[2];
                let double = (second[across] - first[across]) * (third[down] - first[down])
                    - (third[across] - first[across]) * (second[down] - first[down]);
                if double.abs() < PARALLEL {
                    continue;
                }
                let left = ((sample[0] - first[across]) * (third[down] - first[down])
                    - (sample[1] - first[down]) * (third[across] - first[across]))
                    / double;
                let right = ((second[across] - first[across]) * (sample[1] - first[down])
                    - (second[down] - first[down]) * (sample[0] - first[across]))
                    / double;
                let base = 1.0 - left - right;
                if left <= 0.0 || right <= 0.0 || base <= 0.0 {
                    continue;
                }
                crossings.push(left * second[plane] + right * third[plane] + base * first[plane]);
            }
            if crossings.len() < 2 {
                continue;
            }
            crossings.sort_unstable_by(f32::total_cmp);
            let mut cell = [0i64; 3];
            cell[across] = origin[0] + across_index as i64;
            cell[down] = origin[1] + down_index as i64;
            for pair in crossings.as_chunks::<2>().0 {
                let first = (pair[0] - 0.5).floor() as i64 + 1;
                let last = (pair[1] - 0.5).ceil() as i64 - 1;
                for position in first..=last {
                    cell[plane] = position;
                    area.set(cell);
                }
            }
        }
    }
}

fn dominant(triangles: &[Triangle]) -> (usize, usize, usize) {
    let mut low = [f32::INFINITY; 3];
    let mut high = [f32::NEG_INFINITY; 3];
    for triangle in triangles {
        for point in triangle {
            for axis in 0..3 {
                low[axis] = low[axis].min(point[axis]);
                high[axis] = high[axis].max(point[axis]);
            }
        }
    }
    let spread = [0, 1, 2].map(|axis| high[axis] - low[axis]);
    let plane = spread
        .iter()
        .enumerate()
        .max_by(|left, right| left.1.total_cmp(right.1))
        .map_or(0, |pair| pair.0);
    (plane, (plane + 1) % 3, (plane + 2) % 3)
}
