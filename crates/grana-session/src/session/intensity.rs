use glam::Vec3;
use grana_object::{SampleAccumulator, SampleStatistics};
use grana_pipeline::Channels;
use grana_readout::{ImportedFilament, ImportedSpot, ImportedSurface};
use grana_volume::Level;

const LIMIT: u64 = 1 << 22;
const BUDGET: u64 = 32 << 20;

pub(super) struct Sampled {
    pub(super) intensities: Vec<SampleStatistics>,
    pub(super) measured: bool,
}

pub(super) struct Sampler<'a> {
    channels: &'a Channels,
    remaining: u64,
}

impl<'a> Sampler<'a> {
    pub(super) fn new(channels: &'a Channels) -> Self {
        Self {
            channels,
            remaining: BUDGET,
        }
    }

    pub(super) fn surface(
        &mut self,
        time: u32,
        surface: &ImportedSurface,
    ) -> Result<Sampled, String> {
        let positions = surface.vertices();
        if positions.is_empty() || surface.triangles().is_empty() {
            return Err("the surface holds no triangle to sample".to_string());
        }
        let low = [0, 1, 2].map(|axis| {
            positions
                .iter()
                .map(|point| point[axis])
                .fold(f32::INFINITY, f32::min)
        });
        let high = [0, 1, 2].map(|axis| {
            positions
                .iter()
                .map(|point| point[axis])
                .fold(f32::NEG_INFINITY, f32::max)
        });
        self.sample(time, low, high, |level, low, high, mask| {
            surface_mask(level, low, high, positions, surface.triangles(), mask)
        })
    }

    pub(super) fn spot(&mut self, time: u32, spot: &ImportedSpot) -> Result<Sampled, String> {
        let center = spot.center();
        let reach = [0, 1, 2].map(|axis| center[axis] + spot.radius());
        let low = [0, 1, 2].map(|axis| center[axis] - spot.radius());
        self.sample(time, low, reach, |level, low, high, mask| {
            spot_mask(level, low, high, center, spot.radius(), mask)
        })
    }

    pub(super) fn filament(
        &mut self,
        time: u32,
        filament: &ImportedFilament,
    ) -> Result<Sampled, String> {
        let positions = filament.positions();
        let radii = filament.radii();
        if positions.is_empty() || filament.edges().is_empty() {
            return Err("the filament holds no edge to sample".to_string());
        }
        let reach = radii.iter().copied().fold(0.0f32, f32::max);
        let low = [0, 1, 2].map(|axis| {
            positions
                .iter()
                .map(|point| point[axis])
                .fold(f32::INFINITY, f32::min)
                - reach
        });
        let high = [0, 1, 2].map(|axis| {
            positions
                .iter()
                .map(|point| point[axis])
                .fold(f32::NEG_INFINITY, f32::max)
                + reach
        });
        let edges = filament.edges();
        self.sample(time, low, high, |level, low, high, mask| {
            filament_mask(level, low, high, positions, radii, edges, mask)
        })
    }

    fn sample(
        &mut self,
        time: u32,
        low: [f32; 3],
        high: [f32; 3],
        fill: impl FnOnce(Level, [u32; 3], [u32; 3], &mut [bool]),
    ) -> Result<Sampled, String> {
        let channels = self.channels.pyramid().channel_count();
        let per_object = (self.remaining / channels as u64).clamp(1, LIMIT);
        let Some((index, low, high)) = bounds(self.channels, low, high, per_object) else {
            return Err("the object lies outside the dataset".to_string());
        };
        let level = self.channels.pyramid().level(index);
        let dims = [high[0] - low[0], high[1] - low[1], high[2] - low[2]];
        let voxels = u64::from(dims[0]) * u64::from(dims[1]) * u64::from(dims[2]);
        self.remaining = self.remaining.saturating_sub(voxels * channels as u64);
        let mut mask = vec![false; voxels as usize];
        fill(level, low, high, &mut mask);
        let measured = mask.iter().any(|inside| *inside);
        if !measured {
            seed(low, high, &mut mask)?;
        }
        let ranges = [low[0]..high[0], low[1]..high[1], low[2]..high[2]];
        let mut accumulators = vec![SampleAccumulator::new(); channels];
        for (channel, accumulator) in accumulators.iter_mut().enumerate() {
            let data = self
                .channels
                .read_region(time, index, channel, &ranges, [0.0; 3])
                .map_err(|error| error.to_string())?;
            if data.voxel_count() != mask.len() {
                return Err("the sampled region disagrees with its extent".to_string());
            }
            for (slot, inside) in mask.iter().enumerate() {
                if *inside {
                    accumulator.push(data.value(slot));
                }
            }
        }
        Ok(Sampled {
            intensities: accumulators
                .into_iter()
                .map(SampleAccumulator::statistics)
                .collect(),
            measured,
        })
    }
}

fn bounds(
    channels: &Channels,
    low: [f32; 3],
    high: [f32; 3],
    limit: u64,
) -> Option<(usize, [u32; 3], [u32; 3])> {
    let count = channels.pyramid().level_count();
    let mut coarsest = None;
    for index in 0..count {
        let info = channels.pyramid().level(index);
        let Some((low, high)) = voxel_bounds(info, low, high) else {
            continue;
        };
        let voxels = [low[0], low[1], low[2]]
            .into_iter()
            .zip(high)
            .map(|(low, high)| u64::from(high - low))
            .product::<u64>();
        if voxels <= limit {
            return Some((index, low, high));
        }
        coarsest = Some((index, low, high));
    }
    let (index, low, high) = coarsest?;
    let middle = [0, 1, 2].map(|axis| (low[axis] + high[axis]) / 2);
    Some((index, middle, [middle[0] + 1, middle[1] + 1, middle[2] + 1]))
}

fn voxel_bounds(level: Level, low: [f32; 3], high: [f32; 3]) -> Option<([u32; 3], [u32; 3])> {
    let low = level.voxel_of(low);
    let high = level.voxel_of(high);
    let extent = level.extent();
    let limits = [extent.x(), extent.y(), extent.z()];
    let mut start = [0u32; 3];
    let mut end = [0u32; 3];
    for axis in 0..3 {
        let first = low[axis].min(high[axis]).floor();
        let last = low[axis].max(high[axis]).ceil() + 1.0;
        if !(first.is_finite() && last.is_finite()) {
            return None;
        }
        start[axis] = first.max(0.0) as u32;
        end[axis] = (last as i64).clamp(0, i64::from(limits[axis])) as u32;
        if start[axis] >= end[axis] {
            return None;
        }
    }
    Some((start, end))
}

fn surface_mask(
    level: Level,
    low: [u32; 3],
    high: [u32; 3],
    positions: &[[f32; 3]],
    triangles: &[[u32; 3]],
    mask: &mut [bool],
) {
    let dims = [high[0] - low[0], high[1] - low[1], high[2] - low[2]];
    let width = dims[0] as usize;
    let height = dims[1] as usize;
    let mut columns = vec![Vec::<f32>::new(); width * height];
    let voxels = positions
        .iter()
        .map(|point| level.voxel_of(*point))
        .collect::<Vec<_>>();
    for triangle in triangles {
        if triangle.iter().any(|slot| *slot as usize >= voxels.len()) {
            continue;
        }
        let [a, b, c] = triangle.map(|slot| voxels[slot as usize]);
        let area = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
        if area.abs() <= 1.0e-9 {
            continue;
        }
        let first = [0, 1].map(|axis| a[axis].min(b[axis]).min(c[axis]) - 0.5);
        let last = [0, 1].map(|axis| a[axis].max(b[axis]).max(c[axis]) - 0.5);
        let start = [
            (first[0].floor() as i64).max(i64::from(low[0])),
            (first[1].floor() as i64).max(i64::from(low[1])),
        ];
        let end = [
            (last[0].ceil() as i64).min(i64::from(high[0]) - 1),
            (last[1].ceil() as i64).min(i64::from(high[1]) - 1),
        ];
        for y in start[1]..=end[1] {
            let point_y = y as f32 + 0.5;
            for x in start[0]..=end[0] {
                let point_x = x as f32 + 0.5;
                let u =
                    ((point_x - a[0]) * (c[1] - a[1]) - (point_y - a[1]) * (c[0] - a[0])) / area;
                let v =
                    ((b[0] - a[0]) * (point_y - a[1]) - (b[1] - a[1]) * (point_x - a[0])) / area;
                if u < -1.0e-6 || v < -1.0e-6 || u + v > 1.0 + 1.0e-6 {
                    continue;
                }
                let z = a[2] + u * (b[2] - a[2]) + v * (c[2] - a[2]);
                if z.is_finite() {
                    let column =
                        (x - i64::from(low[0])) as usize + width * (y - i64::from(low[1])) as usize;
                    columns[column].push(z);
                }
            }
        }
    }
    for (column, crossings) in columns.iter_mut().enumerate() {
        if crossings.len() < 2 {
            continue;
        }
        crossings.sort_by(f32::total_cmp);
        for pair in crossings.as_chunks::<2>().0 {
            let start = (pair[0] - 0.5).floor() as i64 + 1;
            let end = (pair[1] - 0.5).ceil() as i64 - 1;
            let start = start.max(i64::from(low[2]));
            let end = end.min(i64::from(high[2]) - 1);
            for z in start..=end {
                let index = column + width * height * (z - i64::from(low[2])) as usize;
                mask[index] = true;
            }
        }
    }
}

fn spot_mask(
    level: Level,
    low: [u32; 3],
    high: [u32; 3],
    center: [f32; 3],
    radius: f32,
    mask: &mut [bool],
) {
    let dims = [high[0] - low[0], high[1] - low[1], high[2] - low[2]];
    let width = dims[0] as usize;
    let height = dims[1] as usize;
    let center = Vec3::from(center);
    let reach = radius * radius;
    for z in low[2]..high[2] {
        for y in low[1]..high[1] {
            for x in low[0]..high[0] {
                let world = Vec3::from(level.voxel_center([x as f32, y as f32, z as f32]));
                if world.distance_squared(center) <= reach {
                    let index = (x - low[0]) as usize
                        + width * ((y - low[1]) as usize + height * (z - low[2]) as usize);
                    mask[index] = true;
                }
            }
        }
    }
}

fn filament_mask(
    level: Level,
    low: [u32; 3],
    high: [u32; 3],
    positions: &[[f32; 3]],
    radii: &[f32],
    edges: &[[u32; 2]],
    mask: &mut [bool],
) {
    let dims = [high[0] - low[0], high[1] - low[1], high[2] - low[2]];
    let width = dims[0] as usize;
    let height = dims[1] as usize;
    for edge in edges {
        let (Some(first), Some(second)) = (
            positions.get(edge[0] as usize),
            positions.get(edge[1] as usize),
        ) else {
            continue;
        };
        let (first_radius, second_radius) = (radii[edge[0] as usize], radii[edge[1] as usize]);
        let reach = first_radius.max(second_radius);
        let minimum = [0, 1, 2].map(|axis| first[axis].min(second[axis]) - reach);
        let maximum = [0, 1, 2].map(|axis| first[axis].max(second[axis]) + reach);
        let Some((start, end)) = voxel_range(level, minimum, maximum, low, high) else {
            continue;
        };
        let origin = Vec3::from(*first);
        let direction = Vec3::from(*second) - origin;
        let length = direction.length_squared();
        for z in start[2]..end[2] {
            for y in start[1]..end[1] {
                for x in start[0]..end[0] {
                    let world = Vec3::from(level.voxel_center([x as f32, y as f32, z as f32]));
                    let along = if length > 0.0 {
                        ((world - origin).dot(direction) / length).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    let radius = first_radius + along * (second_radius - first_radius);
                    if world.distance_squared(origin + direction * along) <= radius * radius {
                        let index = (x - low[0]) as usize
                            + width * ((y - low[1]) as usize + height * (z - low[2]) as usize);
                        mask[index] = true;
                    }
                }
            }
        }
    }
}

fn voxel_range(
    level: Level,
    minimum: [f32; 3],
    maximum: [f32; 3],
    low: [u32; 3],
    high: [u32; 3],
) -> Option<([u32; 3], [u32; 3])> {
    let first = level.voxel_of(minimum);
    let last = level.voxel_of(maximum);
    let mut start = [0u32; 3];
    let mut end = [0u32; 3];
    for axis in 0..3 {
        if !(first[axis].is_finite() && last[axis].is_finite()) {
            return None;
        }
        start[axis] =
            (first[axis].floor() as i64).clamp(i64::from(low[axis]), i64::from(high[axis])) as u32;
        end[axis] = (last[axis].ceil() as i64 + 1)
            .clamp(i64::from(low[axis]), i64::from(high[axis])) as u32;
        if start[axis] >= end[axis] {
            return None;
        }
    }
    Some((start, end))
}

fn seed(low: [u32; 3], high: [u32; 3], mask: &mut [bool]) -> Result<(), String> {
    let dims = [high[0] - low[0], high[1] - low[1], high[2] - low[2]];
    if dims.contains(&0) {
        return Err("the sampled region holds no voxel".to_string());
    }
    let middle = [dims[0] / 2, dims[1] / 2, dims[2] / 2];
    let width = dims[0] as usize;
    let height = dims[1] as usize;
    let index = middle[0] as usize + width * (middle[1] as usize + height * middle[2] as usize);
    mask[index] = true;
    Ok(())
}
