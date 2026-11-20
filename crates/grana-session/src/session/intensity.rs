use glam::Vec3;
use grana_geometry::Ellipsoid;
use grana_object::{SampleAccumulator, SampleStatistics};
use grana_pipeline::Channels;
use grana_readout::{ImportedFilament, ImportedSpot, ImportedSurface};
use grana_volume::Level;

const BUDGET: u64 = 32 << 20;
const FLOOR: u64 = 4096;
const LIMIT: u64 = 1 << 22;
const TILE: u32 = 32;

const TILE_VOXELS: u64 = (TILE as u64).pow(3);

pub(super) struct Sampled {
    pub(super) intensities: Vec<SampleStatistics>,
    pub(super) level: usize,
    pub(super) coverage: f32,
    pub(super) measured: bool,
}

pub(super) struct Sampler<'a> {
    channels: &'a Channels,
    per_object: u64,
}

impl<'a> Sampler<'a> {
    pub(super) fn new(channels: &'a Channels, objects: u64) -> Self {
        let per_channel = BUDGET / u64::from(channels.pyramid().channel_count() as u32);
        let per_object = (per_channel / objects.max(1)).clamp(FLOOR, LIMIT);
        Self {
            channels,
            per_object,
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
        let triangles = surface.triangles();
        self.sample(time, low, high, |level, low, high, mask| {
            surface_mask(level, low, high, positions, triangles, mask)
        })
    }

    pub(super) fn spot(&mut self, time: u32, spot: &ImportedSpot) -> Result<Sampled, String> {
        let ellipsoid = Ellipsoid::sphere(spot.center(), spot.radius());
        let (low, high) = ellipsoid.bounds();
        self.sample(time, low, high, |level, low, high, mask| {
            spot_mask(level, low, high, ellipsoid, mask)
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
        mut fill: impl FnMut(Level, [u32; 3], [u32; 3], &mut [bool]),
    ) -> Result<Sampled, String> {
        let channels = self.channels.pyramid().channel_count();
        let allowance = self.per_object;
        let (index, low, high) = self.choose(low, high, allowance)?;
        let level = self.channels.pyramid().level(index);
        let dims = [high[0] - low[0], high[1] - low[1], high[2] - low[2]];
        let tiles = tiles_of(dims);
        let selected = select(tiles, allowance);
        let coverage = selected.len() as f32 / (tiles[0] * tiles[1] * tiles[2]).max(1) as f32;
        let mut accumulators = vec![SampleAccumulator::new(); channels];
        let mut measured = false;
        let mut inside_any = false;
        for tile in selected {
            let origin = [0, 1, 2].map(|axis| low[axis] + tile[axis] * TILE);
            let reach = [0, 1, 2].map(|axis| (low[axis] + (tile[axis] + 1) * TILE).min(high[axis]));
            let extent = [
                reach[0] - origin[0],
                reach[1] - origin[1],
                reach[2] - origin[2],
            ];
            let voxels = u64::from(extent[0]) * u64::from(extent[1]) * u64::from(extent[2]);
            let mut mask = vec![false; voxels as usize];
            fill(level, origin, reach, &mut mask);
            if !mask.iter().any(|inside| *inside) {
                continue;
            }
            inside_any = true;
            let ranges = [
                origin[0]..reach[0],
                origin[1]..reach[1],
                origin[2]..reach[2],
            ];
            for (channel, accumulator) in accumulators.iter_mut().enumerate() {
                let data = self
                    .channels
                    .read_region(time, index, channel, &ranges, &grana_warp::Warp::IDENTITY)
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
        }
        if !inside_any {
            let centre = [0, 1, 2].map(|axis| (low[axis] + high[axis]) / 2);
            let ranges = [
                centre[0]..centre[0] + 1,
                centre[1]..centre[1] + 1,
                centre[2]..centre[2] + 1,
            ];
            for (channel, accumulator) in accumulators.iter_mut().enumerate() {
                let region = self
                    .channels
                    .read_region(time, index, channel, &ranges, &grana_warp::Warp::IDENTITY)
                    .map_err(|error| error.to_string())?;
                accumulator.push(region.value(0));
            }
        } else {
            measured = true;
        }
        Ok(Sampled {
            intensities: accumulators
                .into_iter()
                .map(SampleAccumulator::statistics)
                .collect(),
            level: index,
            coverage: if measured { coverage } else { 0.0 },
            measured,
        })
    }

    fn choose(
        &self,
        low: [f32; 3],
        high: [f32; 3],
        allowance: u64,
    ) -> Result<(usize, [u32; 3], [u32; 3]), String> {
        let pyramid = self.channels.pyramid();
        let allowed = (allowance / TILE_VOXELS).max(1);
        let mut coarsest = None;
        for index in 0..pyramid.level_count() {
            let level = pyramid.level(index);
            let Some((low, high)) = voxel_bounds(level, low, high) else {
                continue;
            };
            let dims = [high[0] - low[0], high[1] - low[1], high[2] - low[2]];
            let tiles = tiles_of(dims);
            let count = u64::from(tiles[0]) * u64::from(tiles[1]) * u64::from(tiles[2]);
            coarsest = Some((index, low, high));
            if count <= allowed {
                return Ok((index, low, high));
            }
        }
        coarsest.ok_or_else(|| "the object lies outside the dataset".to_string())
    }
}

fn tiles_of(dims: [u32; 3]) -> [u32; 3] {
    dims.map(|value| value.div_ceil(TILE).max(1))
}

fn select(tiles: [u32; 3], allowance: u64) -> Vec<[u32; 3]> {
    let count = tiles[0] * tiles[1] * tiles[2];
    let allowed = (allowance / TILE_VOXELS).max(1) as u32;
    if count <= allowed {
        return (0..count)
            .map(|index| {
                [
                    index % tiles[0],
                    (index / tiles[0]) % tiles[1],
                    index / (tiles[0] * tiles[1]),
                ]
            })
            .collect();
    }
    let step = count.div_ceil(allowed);
    (0..count)
        .step_by(step as usize)
        .map(|index| {
            [
                index % tiles[0],
                (index / tiles[0]) % tiles[1],
                index / (tiles[0] * tiles[1]),
            ]
        })
        .collect()
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

fn spot_mask(level: Level, low: [u32; 3], high: [u32; 3], ellipsoid: Ellipsoid, mask: &mut [bool]) {
    let dims = [high[0] - low[0], high[1] - low[1], high[2] - low[2]];
    let width = dims[0] as usize;
    let height = dims[1] as usize;
    for z in low[2]..high[2] {
        for y in low[1]..high[1] {
            for x in low[0]..high[0] {
                let world = level.voxel_center([x as f32, y as f32, z as f32]);
                if ellipsoid.contains(world) {
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
