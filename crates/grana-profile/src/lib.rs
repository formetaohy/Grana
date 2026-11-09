use std::ops::Range;

use glam::Vec3;
use grana_volume::{Extent, VoxelData};

const SAMPLE_LIMIT: usize = 4096;
const CHUNK_SPAN: f32 = 64.0;
const PADDING: u32 = 1;
const FLOOR: f32 = 1.0e-6;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Line {
    from: [f32; 3],
    to: [f32; 3],
}

impl Line {
    pub fn new(from: [f32; 3], to: [f32; 3]) -> Self {
        assert!(
            from.iter().chain(&to).all(|value| value.is_finite()),
            "a profile line spans finite world points"
        );
        assert!(from != to, "a profile line spans two distinct world points");
        Self { from, to }
    }

    pub fn from(self) -> [f32; 3] {
        self.from
    }

    pub fn to(self) -> [f32; 3] {
        self.to
    }

    pub fn length(self) -> f32 {
        Vec3::from(self.to).distance(Vec3::from(self.from))
    }

    pub fn direction(self) -> Vec3 {
        (Vec3::from(self.to) - Vec3::from(self.from)) / self.length()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Sample {
    offset: f32,
    values: Vec<f32>,
}

impl Sample {
    pub fn new(offset: f32, values: Vec<f32>) -> Self {
        assert!(
            offset.is_finite() && offset >= 0.0,
            "a profile sample stands at a non-negative distance"
        );
        Self { offset, values }
    }

    pub fn offset(&self) -> f32 {
        self.offset
    }

    pub fn values(&self) -> &[f32] {
        &self.values
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Profile {
    line: Line,
    spacing: f32,
    names: Vec<String>,
    samples: Vec<Sample>,
}

impl Profile {
    pub fn new(line: Line, spacing: f32, names: Vec<String>, samples: Vec<Sample>) -> Self {
        assert!(
            spacing.is_finite() && spacing > 0.0,
            "a profile samples at a positive spacing"
        );
        assert!(!names.is_empty(), "a profile carries at least one channel");
        assert!(!samples.is_empty(), "a profile carries at least one sample");
        assert!(
            samples
                .iter()
                .all(|sample| sample.values.len() == names.len()),
            "every profile sample measures every channel"
        );
        assert!(
            samples
                .windows(2)
                .all(|pair| pair[0].offset <= pair[1].offset),
            "profile samples stand in order along the line"
        );
        Self {
            line,
            spacing,
            names,
            samples,
        }
    }

    pub fn line(&self) -> Line {
        self.line
    }

    pub fn spacing(&self) -> f32 {
        self.spacing
    }

    pub fn names(&self) -> &[String] {
        &self.names
    }

    pub fn samples(&self) -> &[Sample] {
        &self.samples
    }

    pub fn length(&self) -> f32 {
        self.samples.last().map_or(0.0, |sample| sample.offset)
    }

    pub fn range(&self, channel: usize) -> Option<(f32, f32)> {
        let mut low = f32::INFINITY;
        let mut high = f32::NEG_INFINITY;
        for sample in &self.samples {
            let value = *sample.values.get(channel)?;
            if value.is_finite() {
                low = low.min(value);
                high = high.max(value);
            }
        }
        (low <= high).then_some((low, high))
    }
}

pub trait Source {
    fn extent(&self) -> Extent;

    fn voxel(&self) -> [f32; 3];

    fn read(&self, channel: usize, ranges: &[Range<u32>; 3]) -> Result<VoxelData, String>;
}

pub fn sample(
    source: &impl Source,
    line: Line,
    spacing: f32,
    names: &[String],
) -> Result<Profile, String> {
    if names.is_empty() {
        return Err("a profile samples at least one channel".to_string());
    }
    let extent = source.extent();
    let voxel = source.voxel();
    if voxel
        .iter()
        .any(|value| !value.is_finite() || *value <= 0.0)
    {
        return Err("the dataset declares a positive voxel size".to_string());
    }
    let length = line.length();
    let requested = spacing.max(FLOOR);
    let mut step = requested;
    let mut count = ((length / step).ceil() as usize + 1).max(2);
    if count > SAMPLE_LIMIT {
        count = SAMPLE_LIMIT;
        step = length / (count - 1) as f32;
    }
    let direction = line.direction();
    let origin = Vec3::from(line.from());
    let half = Vec3::new(
        0.5 * extent.x() as f32 * voxel[0],
        0.5 * extent.y() as f32 * voxel[1],
        0.5 * extent.z() as f32 * voxel[2],
    );
    let size = Vec3::new(voxel[0], voxel[1], voxel[2]);
    let limits = Vec3::new(extent.x() as f32, extent.y() as f32, extent.z() as f32);
    let offsets = (0..count)
        .map(|index| (index as f32 * step).min(length))
        .collect::<Vec<_>>();
    let points = offsets
        .iter()
        .map(|offset| {
            let world = origin + direction * *offset;
            let index = (world + half) / size - Vec3::splat(0.5);
            index.clamp(Vec3::ZERO, limits - Vec3::ONE)
        })
        .collect::<Vec<_>>();
    let groups = groups(&points);
    let mut values = vec![vec![0.0f32; names.len()]; count];
    for group in groups {
        let ranges = ranges(&points, &group, &limits)?;
        for (channel, _) in names.iter().enumerate() {
            let data = source.read(channel, &ranges)?;
            for index in &group {
                let local = points[*index]
                    - Vec3::new(
                        ranges[0].start as f32,
                        ranges[1].start as f32,
                        ranges[2].start as f32,
                    );
                values[*index][channel] = interpolate(&data, &ranges, local)?;
            }
        }
    }
    let samples = offsets
        .into_iter()
        .zip(values)
        .map(|(offset, values)| Sample::new(offset, values))
        .collect::<Vec<_>>();
    Ok(Profile::new(line, step, names.to_vec(), samples))
}

fn groups(points: &[Vec3]) -> Vec<Vec<usize>> {
    let mut groups = Vec::new();
    let mut current = Vec::new();
    let mut low = Vec3::splat(f32::INFINITY);
    let mut high = Vec3::splat(f32::NEG_INFINITY);
    for (index, point) in points.iter().enumerate() {
        let next_low = low.min(*point);
        let next_high = high.max(*point);
        if !current.is_empty() && (next_high - next_low).max_element() > CHUNK_SPAN {
            groups.push(std::mem::take(&mut current));
            low = *point;
            high = *point;
        } else {
            low = next_low;
            high = next_high;
        }
        current.push(index);
    }
    if !current.is_empty() {
        groups.push(current);
    }
    groups
}

fn ranges(points: &[Vec3], group: &[usize], limits: &Vec3) -> Result<[Range<u32>; 3], String> {
    let mut low = Vec3::splat(f32::INFINITY);
    let mut high = Vec3::splat(f32::NEG_INFINITY);
    for index in group {
        low = low.min(points[*index]);
        high = high.max(points[*index]);
    }
    let ranges: [Range<u32>; 3] = std::array::from_fn(|axis| {
        let start = (low[axis].floor().max(0.0) as u32).saturating_sub(PADDING);
        let end = (high[axis].ceil().max(0.0) as u32)
            .saturating_add(PADDING + 1)
            .min(limits[axis] as u32);
        start..end
    });
    if ranges.iter().any(|range| range.start >= range.end) {
        return Err("a profile segment falls outside the dataset".to_string());
    }
    Ok(ranges)
}

fn interpolate(data: &VoxelData, ranges: &[Range<u32>; 3], local: Vec3) -> Result<f32, String> {
    let size = [
        ranges[0].end - ranges[0].start,
        ranges[1].end - ranges[1].start,
        ranges[2].end - ranges[2].start,
    ];
    if size.contains(&0) {
        return Err("a profile segment covers no voxel".to_string());
    }
    let within = |axis: usize| local[axis].clamp(0.0, (size[axis] - 1) as f32);
    let x = within(0);
    let y = within(1);
    let z = within(2);
    let floor = |value: f32, limit: u32| {
        let low = value.floor().max(0.0) as u32;
        low.min(limit.saturating_sub(1))
    };
    let x0 = floor(x, size[0]);
    let y0 = floor(y, size[1]);
    let z0 = floor(z, size[2]);
    let x1 = (x0 + 1).min(size[0] - 1);
    let y1 = (y0 + 1).min(size[1] - 1);
    let z1 = (z0 + 1).min(size[2] - 1);
    let fx = x - x0 as f32;
    let fy = y - y0 as f32;
    let fz = z - z0 as f32;
    let stride = |z: u32, y: u32, x: u32| (x + y * size[0] + z * size[0] * size[1]) as usize;
    let corner = |x: u32, y: u32, z: u32| data.value(stride(z, y, x));
    let c00 = corner(x0, y0, z0) * (1.0 - fx) + corner(x1, y0, z0) * fx;
    let c10 = corner(x0, y1, z0) * (1.0 - fx) + corner(x1, y1, z0) * fx;
    let c01 = corner(x0, y0, z1) * (1.0 - fx) + corner(x1, y0, z1) * fx;
    let c11 = corner(x0, y1, z1) * (1.0 - fx) + corner(x1, y1, z1) * fx;
    let c0 = c00 * (1.0 - fy) + c10 * fy;
    let c1 = c01 * (1.0 - fy) + c11 * fy;
    Ok(c0 * (1.0 - fz) + c1 * fz)
}
