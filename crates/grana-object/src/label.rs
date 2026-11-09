use std::ops::Range;

use grana_geometry::PackedMesh;
use grana_volume::Level;

use crate::Shape;

const NUDGE: [f32; 2] = [1.0e-4, 3.7e-4];
const PARALLEL: f32 = 1.0e-12;
const LABEL_LIMIT: usize = u16::MAX as usize;
const UNIT: f32 = 65535.0;

pub struct LabelRaster<'a> {
    shape: &'a Shape,
    level: Level,
    extent: [u32; 3],
    bounds: Vec<[Range<u32>; 3]>,
    planes: Vec<[usize; 3]>,
}

impl<'a> LabelRaster<'a> {
    pub fn new(shape: &'a Shape, level: Level) -> Self {
        let objects = shape.objects();
        assert!(
            objects <= LABEL_LIMIT,
            "a label image addresses at most {LABEL_LIMIT} objects"
        );
        let extent = level.extent();
        let extent = [extent.x(), extent.y(), extent.z()];
        let mut bounds = Vec::with_capacity(objects);
        let mut planes = Vec::with_capacity(objects);
        match shape {
            Shape::Spots(spots) => {
                for spot in spots.iter() {
                    let center = spot.center();
                    let radius = spot.radius();
                    let low = [0, 1, 2].map(|axis| center[axis] - radius);
                    let high = [0, 1, 2].map(|axis| center[axis] + radius);
                    bounds.push(cells(low, high, level, extent));
                    planes.push([0, 1, 2]);
                }
            }
            Shape::Mesh(mesh) => {
                for group in mesh.groups() {
                    let low = group.min();
                    let high = [0, 1, 2].map(|axis| low[axis] + UNIT * group.scale()[axis]);
                    let cells = cells(low, high, level, extent);
                    let spread = [0, 1, 2].map(|axis| cells[axis].end - cells[axis].start);
                    let plane = spread
                        .iter()
                        .enumerate()
                        .max_by_key(|(_, value)| **value)
                        .map_or(0, |(axis, _)| axis);
                    bounds.push(cells);
                    planes.push([plane, (plane + 1) % 3, (plane + 2) % 3]);
                }
            }
        }
        Self {
            shape,
            level,
            extent,
            bounds,
            planes,
        }
    }

    pub fn objects(&self) -> usize {
        self.bounds.len()
    }

    pub fn render(&self, x: Range<u32>, y: Range<u32>, z: Range<u32>) -> Vec<u16> {
        self.render_visible(None, x, y, z)
    }

    pub fn render_visible(
        &self,
        keep: Option<&[bool]>,
        x: Range<u32>,
        y: Range<u32>,
        z: Range<u32>,
    ) -> Vec<u16> {
        let origin = [x.start, y.start, z.start];
        let ranges = [x, y, z];
        assert!(
            (0..3).all(|axis| {
                ranges[axis].start < ranges[axis].end && ranges[axis].end <= self.extent[axis]
            }),
            "a label chunk lies inside the level it rasterizes"
        );
        let extent = [0, 1, 2].map(|axis| ranges[axis].end - ranges[axis].start);
        let mut labels =
            vec![0u16; (extent[0] as usize) * (extent[1] as usize) * (extent[2] as usize)];
        let area = Area {
            origin,
            extent,
            level: self.level,
        };
        for (object, bounds) in self.bounds.iter().enumerate() {
            if keep.is_some_and(|keep| !keep.get(object).copied().unwrap_or(false)) {
                continue;
            }
            let overlap = [0, 1, 2].map(|axis| {
                bounds[axis].start.max(ranges[axis].start)..bounds[axis].end.min(ranges[axis].end)
            });
            if (0..3).any(|axis| overlap[axis].start >= overlap[axis].end) {
                continue;
            }
            let label = (object + 1) as u16;
            match self.shape {
                Shape::Spots(spots) => area.sphere(
                    spots[object].center(),
                    spots[object].radius(),
                    label,
                    &mut labels,
                ),
                Shape::Mesh(mesh) => self.mesh(mesh, object, overlap, &area, label, &mut labels),
            }
        }
        labels
    }

    fn mesh(
        &self,
        mesh: &PackedMesh,
        object: usize,
        overlap: [Range<u32>; 3],
        area: &Area,
        label: u16,
        labels: &mut [u16],
    ) {
        let group = mesh.groups()[object];
        let count = group.index_count() as usize / 3;
        if count == 0 {
            return;
        }
        let [plane, across, down] = self.planes[object];
        let mut triangles = Vec::with_capacity(count);
        let mut low = [i64::MAX; 2];
        let mut high = [i64::MIN; 2];
        for index in 0..count {
            let triangle = mesh
                .triangle(object, index)
                .map(|point| area.continuous(point));
            for (slot, axis) in [across, down].into_iter().enumerate() {
                for point in triangle {
                    let cell = point[axis].floor() as i64;
                    low[slot] = low[slot].min(cell);
                    high[slot] = high[slot].max(cell);
                }
            }
            triangles.push(triangle);
        }
        let mut origin = [0i64; 2];
        let mut limit = [0i64; 2];
        for (slot, axis) in [across, down].into_iter().enumerate() {
            origin[slot] = (low[slot]).max(i64::from(overlap[axis].start));
            limit[slot] = (high[slot] + 1).min(i64::from(overlap[axis].end));
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
                    crossings
                        .push(left * second[plane] + right * third[plane] + base * first[plane]);
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
                        area.set(cell, label, labels);
                    }
                }
            }
        }
    }
}

fn cells(low: [f32; 3], high: [f32; 3], level: Level, extent: [u32; 3]) -> [Range<u32>; 3] {
    let low = level.voxel_of(low);
    let high = level.voxel_of(high);
    [0, 1, 2].map(|axis| {
        let start = (low[axis].floor() as i64).clamp(0, i64::from(extent[axis])) as u32;
        let end = ((high[axis].ceil() as i64) + 1).clamp(0, i64::from(extent[axis])) as u32;
        start..end
    })
}

struct Area {
    origin: [u32; 3],
    extent: [u32; 3],
    level: Level,
}

impl Area {
    fn continuous(&self, point: [f32; 3]) -> [f32; 3] {
        self.level.voxel_of(point)
    }

    fn sphere(&self, center: [f32; 3], radius: f32, label: u16, labels: &mut [u16]) {
        if !(radius.is_finite() && radius > 0.0) {
            return;
        }
        let continuous = self.continuous(center);
        let spacing = self.level.voxel();
        let mut low = [0u32; 3];
        let mut high = [0u32; 3];
        for axis in 0..3 {
            let reach = radius / spacing[axis];
            low[axis] = ((continuous[axis] - reach).floor() as i64)
                .max(i64::from(self.origin[axis])) as u32;
            high[axis] = (((continuous[axis] + reach).ceil() as i64) + 1)
                .min(i64::from(self.origin[axis] + self.extent[axis]))
                as u32;
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
                    let point = self.level.voxel_center(cell.map(|value| value as f32));
                    let offset = [0, 1, 2].map(|axis| point[axis] - center[axis]);
                    if offset.iter().map(|value| value * value).sum::<f32>() <= radius * radius {
                        self.set(cell.map(i64::from), label, labels);
                    }
                    cell[0] += 1;
                }
                cell[1] += 1;
            }
            cell[2] += 1;
        }
    }

    fn set(&self, cell: [i64; 3], label: u16, labels: &mut [u16]) {
        let mut local = [0usize; 3];
        for axis in 0..3 {
            let value = cell[axis] - i64::from(self.origin[axis]);
            if value < 0 || value >= i64::from(self.extent[axis]) {
                return;
            }
            local[axis] = value as usize;
        }
        let index =
            local[0] + self.extent[0] as usize * (local[1] + self.extent[1] as usize * local[2]);
        labels[index] = label;
    }
}
