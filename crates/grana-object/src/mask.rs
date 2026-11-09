use glam::Vec3;
use grana_geometry::PackedMesh;
use grana_volume::{Level, Region};

use crate::Shape;

const NUDGE: [f32; 2] = [1.0e-4, 3.7e-4];
const WORD: usize = 64;
const PARALLEL: f32 = 1.0e-12;
const DILATE_LIMIT: u64 = 1 << 28;
const FAR: f32 = 1.0e18;

type Triangle = [[f32; 3]; 3];

#[derive(Clone)]
pub struct Mask {
    origin: [i64; 3],
    extent: [u32; 3],
    bits: Vec<u64>,
    level: Level,
}

impl Mask {
    pub fn empty(level: Level) -> Self {
        Self {
            origin: [0; 3],
            extent: [0; 3],
            bits: Vec::new(),
            level,
        }
    }

    pub fn boxed(region: Region, level: Level) -> Self {
        let Some(ranges) = region.voxel_range(level) else {
            return Self::empty(level);
        };
        let origin = [0, 1, 2].map(|axis| ranges[axis].start as i64);
        let limit = [0, 1, 2].map(|axis| ranges[axis].end as i64);
        let mut area = Area::new(origin, limit, level);
        area.boxed(region);
        area.finish()
    }

    pub fn intersected(&self, other: &Mask) -> Mask {
        assert_eq!(
            self.level, other.level,
            "an analysis mask meets a mask of the same level"
        );
        let origin = [0, 1, 2].map(|axis| self.origin[axis].max(other.origin[axis]));
        let limit = [0, 1, 2].map(|axis| {
            (self.origin[axis] + i64::from(self.extent[axis]))
                .min(other.origin[axis] + i64::from(other.extent[axis]))
        });
        if (0..3).any(|axis| limit[axis] <= origin[axis]) {
            return Mask::empty(self.level);
        }
        let extent = [0, 1, 2].map(|axis| (limit[axis] - origin[axis]) as u32);
        let cells = extent
            .iter()
            .map(|value| u64::from(*value))
            .product::<u64>();
        let mut bits = vec![0u64; cells.div_ceil(WORD as u64) as usize];
        let mut cell = origin;
        while cell[2] < limit[2] {
            cell[1] = origin[1];
            while cell[1] < limit[1] {
                cell[0] = origin[0];
                while cell[0] < limit[0] {
                    if self.contains_cell_index(&cell) && other.contains_cell_index(&cell) {
                        let local = [0, 1, 2].map(|axis| (cell[axis] - origin[axis]) as usize);
                        let index = local[0]
                            + extent[0] as usize * (local[1] + extent[1] as usize * local[2]);
                        bits[index / WORD] |= 1 << (index % WORD);
                    }
                    cell[0] += 1;
                }
                cell[1] += 1;
            }
            cell[2] += 1;
        }
        Mask {
            origin,
            extent,
            bits,
            level: self.level,
        }
    }

    pub fn contains(&self, point: [f32; 3]) -> bool {
        let continuous = self.level.voxel_of(point);
        let cell = continuous.map(|value| value.floor() as i64);
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

    pub fn bounds(&self) -> [std::ops::Range<i64>; 3] {
        [0, 1, 2].map(|axis| self.origin[axis]..self.origin[axis] + i64::from(self.extent[axis]))
    }

    pub fn is_empty(&self) -> bool {
        self.extent.contains(&0)
    }

    pub fn dilated(&self, radius: f32) -> Result<Mask, String> {
        assert!(
            radius.is_finite() && radius >= 0.0,
            "an analysis mask grows by a finite radius"
        );
        if radius == 0.0 {
            return Ok(self.clone());
        }
        if self.is_empty() {
            return Ok(self.clone());
        }
        let voxels = self
            .extent
            .iter()
            .map(|value| u64::from(*value))
            .product::<u64>();
        if voxels > DILATE_LIMIT {
            return Err(format!(
                "the analysis mask holds {voxels} voxels and cannot grow by {radius} µm"
            ));
        }
        let mut field = vec![FAR; voxels as usize];
        for (index, word) in self.bits.iter().enumerate() {
            let mut bits = *word;
            while bits != 0 {
                let slot = index * WORD + bits.trailing_zeros() as usize;
                if let Some(value) = field.get_mut(slot) {
                    *value = 0.0;
                }
                bits &= bits - 1;
            }
        }
        let dims = self.extent.map(|value| value as usize);
        let spacing = self.level.voxel();
        for (axis, spacing) in spacing.into_iter().enumerate() {
            transform(&mut field, dims, axis, spacing * spacing);
        }
        let reach = radius * radius;
        let mut bits = vec![0u64; self.bits.len()];
        for (index, value) in field.iter().enumerate() {
            if *value <= reach {
                bits[index / WORD] |= 1 << (index % WORD);
            }
        }
        Ok(Mask {
            origin: self.origin,
            extent: self.extent,
            bits,
            level: self.level,
        })
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
    frames: usize,
    radius: f32,
    shape: Box<dyn Fn(u32) -> Result<Option<Shape>, String> + Send + Sync>,
}

impl Roi {
    pub fn new(frames: Vec<Option<Shape>>) -> Self {
        let count = frames.len();
        Self {
            frames: count,
            radius: 0.0,
            shape: Box::new(move |time| Ok(frames.get(time as usize).cloned().unwrap_or_default())),
        }
    }

    pub fn paged(
        frames: usize,
        shape: impl Fn(u32) -> Result<Option<Shape>, String> + Send + Sync + 'static,
    ) -> Self {
        Self {
            frames,
            radius: 0.0,
            shape: Box::new(shape),
        }
    }

    pub fn with_radius(mut self, radius: f32) -> Self {
        assert!(
            radius.is_finite() && radius >= 0.0,
            "an analysis mask grows by a finite radius"
        );
        self.radius = radius;
        self
    }

    pub fn frames(&self) -> usize {
        self.frames
    }

    pub fn mask(&self, level: Level, time: u32) -> Result<Mask, String> {
        let Some(frame) = (self.shape)(time)? else {
            return Ok(Mask::empty(level));
        };
        let Some((low, high)) = frame.bounds() else {
            return Ok(Mask::empty(level));
        };
        let spacing = level.spacing();
        let spacing = [spacing.x(), spacing.y(), spacing.z()];
        let extent = level.extent();
        let extent = [extent.x(), extent.y(), extent.z()];
        let continuous = |point: [f32; 3], axis: usize| level.voxel_of(point)[axis];
        let mut origin = [0i64; 3];
        let mut limit = [0i64; 3];
        let margin = [0, 1, 2].map(|axis| {
            if self.radius > 0.0 {
                (self.radius / spacing[axis]).ceil() as i64 + 1
            } else {
                0
            }
        });
        for (axis, slot) in extent.iter().enumerate() {
            origin[axis] = (continuous(low, axis).floor() as i64 - 1 - margin[axis]).max(0);
            limit[axis] =
                (continuous(high, axis).ceil() as i64 + 1 + margin[axis]).min(i64::from(*slot));
        }
        if (0..3).any(|axis| limit[axis] <= origin[axis]) {
            return Ok(Mask::empty(level));
        }
        let mut area = Area::new(origin, limit, level);
        match frame {
            Shape::Spots(spots) => {
                for spot in spots.iter() {
                    area.sphere(spot.center(), spot.radius());
                }
            }
            Shape::Mesh(mesh) => rasterize(&mesh, &mut area),
        }
        let mask = area.finish();
        if self.radius > 0.0 {
            mask.dilated(self.radius)
        } else {
            Ok(mask)
        }
    }
}

struct Area {
    origin: [i64; 3],
    extent: [u32; 3],
    bits: Vec<u64>,
    level: Level,
}

impl Area {
    fn new(origin: [i64; 3], limit: [i64; 3], level: Level) -> Self {
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
            level,
        }
    }

    fn continuous(&self, point: [f32; 3]) -> [f32; 3] {
        self.level.voxel_of(point)
    }

    fn sphere(&mut self, center: [f32; 3], radius: f32) {
        if !(radius.is_finite() && radius > 0.0) {
            return;
        }
        let center = self.continuous(center);
        let spacing = self.level.voxel();
        let mut low = [0i64; 3];
        let mut high = [0i64; 3];
        for axis in 0..3 {
            let reach = radius / spacing[axis];
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
                    let offset = [0, 1, 2]
                        .map(|axis| ((cell[axis] as f32 + 0.5) - center[axis]) * spacing[axis]);
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

    fn boxed(&mut self, region: Region) {
        if self.extent.contains(&0) {
            return;
        }
        let spacing = self.level.voxel();
        let origin = self.level.origin();
        let corner = Vec3::new(
            origin[0] + 0.5 * spacing[0],
            origin[1] + 0.5 * spacing[1],
            origin[2] + 0.5 * spacing[2],
        );
        let half = region.half();
        let center = region.center();
        let width = self.extent[0] as i64;
        let height = self.extent[1] as i64;
        let depth = self.extent[2] as i64;
        let axes = region.axes();
        for z in 0..depth {
            for y in 0..height {
                let row = corner
                    + Vec3::new(
                        0.0,
                        (self.origin[1] + y) as f32 * spacing[1],
                        (self.origin[2] + z) as f32 * spacing[2],
                    );
                let mut low = f32::NEG_INFINITY;
                let mut high = f32::INFINITY;
                for axis in 0..3 {
                    let unit = axes[axis];
                    let offset = (row - center).dot(unit);
                    let slope = spacing[0] * unit.x;
                    if slope.abs() < PARALLEL {
                        if offset.abs() > half[axis] {
                            low = 1.0;
                            high = 0.0;
                            break;
                        }
                        continue;
                    }
                    let first = (-half[axis] - offset) / slope;
                    let last = (half[axis] - offset) / slope;
                    low = low.max(first.min(last));
                    high = high.min(first.max(last));
                }
                if low > high {
                    continue;
                }
                let first = (low.ceil() as i64).max(self.origin[0]);
                let last = (high.floor() as i64).min(self.origin[0] + width - 1);
                if first > last {
                    continue;
                }
                let base = (first - self.origin[0]) as usize
                    + self.extent[0] as usize * (y as usize + height as usize * z as usize);
                set_bits(&mut self.bits, base, (last - first + 1) as usize);
            }
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
            level: self.level,
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

fn set_bits(bits: &mut [u64], start: usize, count: usize) {
    let end = start + count;
    let mut index = start;
    while index < end {
        let word = index / WORD;
        let offset = index % WORD;
        let take = (WORD - offset).min(end - index);
        let mask = if take == WORD {
            u64::MAX
        } else {
            ((1u64 << take) - 1) << offset
        };
        bits[word] |= mask;
        index += take;
    }
}

fn transform(field: &mut [f32], dims: [usize; 3], axis: usize, weight: f32) {
    let [width, height, depth] = dims;
    let plane = width * height;
    let mut line = vec![0.0f32; dims[axis]];
    match axis {
        0 => {
            for z in 0..depth {
                for y in 0..height {
                    let start = z * plane + y * width;
                    line.copy_from_slice(&field[start..start + width]);
                    transform_line(&mut line, weight);
                    field[start..start + width].copy_from_slice(&line);
                }
            }
        }
        1 => {
            for z in 0..depth {
                for x in 0..width {
                    for y in 0..height {
                        line[y] = field[z * plane + y * width + x];
                    }
                    transform_line(&mut line, weight);
                    for y in 0..height {
                        field[z * plane + y * width + x] = line[y];
                    }
                }
            }
        }
        2 => {
            for y in 0..height {
                for x in 0..width {
                    for z in 0..depth {
                        line[z] = field[z * plane + y * width + x];
                    }
                    transform_line(&mut line, weight);
                    for z in 0..depth {
                        field[z * plane + y * width + x] = line[z];
                    }
                }
            }
        }
        _ => unreachable!("a mask has three axes"),
    }
}

fn transform_line(line: &mut [f32], weight: f32) {
    let length = line.len();
    if length < 2 {
        return;
    }
    let mut locations = vec![0usize; length];
    let mut boundaries = vec![0.0f32; length + 1];
    let mut last = 0usize;
    boundaries[0] = f32::NEG_INFINITY;
    boundaries[1] = f32::INFINITY;
    for position in 1..length {
        let mut boundary = crossing(line, &locations, last, position, weight);
        while last > 0 && boundary <= boundaries[last] {
            last -= 1;
            boundary = crossing(line, &locations, last, position, weight);
        }
        last += 1;
        locations[last] = position;
        boundaries[last] = boundary;
        boundaries[last + 1] = f32::INFINITY;
    }
    let mut slot = 0usize;
    for position in 0..length {
        while boundaries[slot + 1] < position as f32 {
            slot += 1;
        }
        let distance = position as f32 - locations[slot] as f32;
        line[position] = distance * distance * weight + line[locations[slot]];
    }
}

fn crossing(line: &[f32], locations: &[usize], slot: usize, position: usize, weight: f32) -> f32 {
    let anchor = locations[slot];
    ((line[position] + weight * (position * position) as f32)
        - (line[anchor] + weight * (anchor * anchor) as f32))
        / (2.0 * weight * (position as f32 - anchor as f32))
}
